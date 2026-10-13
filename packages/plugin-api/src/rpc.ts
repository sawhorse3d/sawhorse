// The wire protocol between the app (the host) and one plugin's Web Worker. Every message
// is plain data: JSON values plus Uint8Array. Since lane 16 every message is serialized once
// by its sender into one transferred ArrayBuffer (wire.ts) and read back by the receiver
// with every limit applied before anything is allocated: the receiver never relies on the
// sender's checks.
//
//   host -> plugin   init, res (an answer to a req), call (run a callback: a command, a
//                    panel event, a tool event; answered by done), evt (an event the plugin
//                    subscribed to), ping (the watchdog)
//   plugin -> host   ready / fail (after the entry ran), req (an API call, answered by res),
//                    done, pong, log
//
// Requests carry ids so answers can arrive in any order. The host enforces permissions on
// every req; the plugin side only shapes calls. A message that cannot be read is answered,
// never dropped: a refused req gets a res with code "too_large" or "invalid", a refused res
// settles the plugin's pending call with the same codes (PluginApiError).

import { decodeFrame, encodeFrame, ProtocolError, readHeader, WIRE_LIMITS, WireError, type FrameHeader, type FrameKind } from "./wire.ts";

export { WireError, WIRE_LIMITS, FRAME_BUDGET, MAX_FRAME, encodeValue, Encoded } from "./wire.ts";
export type { FrameHeader } from "./wire.ts";

export const PROTOCOL = 1;

/** Largest string, byte array, key, value count and nesting the codec accepts (wire.ts). */
export const LIMITS = { ...WIRE_LIMITS, total: 4 * 1024 * 1024 };

export type Json = null | boolean | number | string | Json[] | { [k: string]: Json };
/** Plain data: what may cross the boundary. */
export type Data = null | boolean | number | string | Uint8Array | Data[] | { [k: string]: Data };

export interface PluginInfo {
  id: string;
  name: string;
  version: string;
  permissions: string[];
}

export type HostMessage =
  | {
      t: "init";
      protocol: number;
      plugin: PluginInfo;
      /** Module sources by path inside the plugin ("main.js", "lib/util.js"). */
      files: Record<string, string>;
      entry: string;
      /** The document's display units ("frac-in", "mm" ...). */
      units: string;
    }
  | { t: "res"; id: number; ok: true; value: Data; method?: string }
  | { t: "res"; id: number; ok: false; error: string; code?: string; method?: string }
  | { t: "call"; id: number; fn: string; args: Data[] }
  | { t: "evt"; name: string; payload: Data }
  | { t: "ping"; n: number };

export type LogLevel = "debug" | "log" | "info" | "warn" | "error";

export type PluginMessage =
  | { t: "ready" }
  | { t: "fail"; error: string }
  | { t: "req"; id: number; method: string; params: Data }
  | { t: "done"; id: number; ok: true; value: Data }
  | { t: "done"; id: number; ok: false; error: string }
  | { t: "pong"; n: number }
  | { t: "log"; level: LogLevel; text: string };

/**
 * Lane 15: what every refused API call rejects with: the host's refusal (its `code`:
 * "permission", "invalid", "not_found", "busy" ...), a parameter the runtime could not send
 * ("invalid"), or a message that could not be sent at all ("send_failed").
 */
export class PluginApiError extends Error {
  readonly code: string;
  constructor(message: string, code = "error") {
    super(message);
    this.name = "PluginApiError";
    this.code = code;
  }
}

/** The events a plugin can listen to (API 1). */
export const EVENT_NAMES = ["selectionChanged", "documentChanged", "documentOpened", "toolChanged", "unitsChanged"] as const;

export { ProtocolError };

const LEVELS: LogLevel[] = ["debug", "log", "info", "warn", "error"];

/**
 * Check that `v` is plain data (JSON values, finite numbers, Uint8Array), within the size
 * limits: per value and for the whole (strings, bytes and a 16-byte charge per value add up
 * to at most LIMITS.total). Byte arrays are measured on their backing buffer (a structured
 * clone copies the whole buffer), lists must have no holes or extra properties, keys are at
 * most 256 characters. Throws a ProtocolError naming where the first problem is; the path is
 * built only then. (Messages themselves go through wire.ts; this is for values a structured
 * clone still carries, and for tests.)
 */
export function checkData(v: unknown, where = "value", total: number = LIMITS.total): Data {
  let nodes = 0;
  let spent = 0;
  const path: (string | number)[] = [];
  const at = () => path.reduce<string>((s, p) => (typeof p === "number" ? `${s}[${p}]` : `${s}.${p.length > 40 ? `${p.slice(0, 40)}…` : p}`), where);
  const fail = (m: string): never => {
    throw new ProtocolError(`${at()} ${m}`);
  };
  const spend = (n: number) => {
    spent += n;
    if (spent > total) throw new ProtocolError(`${where} is too large (more than ${Math.round(total / 1024)} KiB in all)`);
  };
  const seen = new Set<object>();
  const walk = (x: unknown, depth: number): void => {
    if (++nodes > LIMITS.nodes) throw new ProtocolError(`${where} is too large (more than ${LIMITS.nodes} values)`);
    spend(16);
    if (depth > LIMITS.depth) fail(`is nested more than ${LIMITS.depth} deep`);
    if (x === null || typeof x === "boolean") return;
    if (typeof x === "number") {
      if (!Number.isFinite(x)) fail(`is not a finite number (${x})`);
      return;
    }
    if (typeof x === "string") {
      if (x.length > LIMITS.string) fail(`is a string longer than ${LIMITS.string} characters`);
      return spend(x.length);
    }
    if (x instanceof Uint8Array) {
      const buf = x.buffer as ArrayBuffer & { resizable?: boolean };
      if (typeof SharedArrayBuffer !== "undefined" && buf instanceof SharedArrayBuffer) fail("is a view of a SharedArrayBuffer");
      if (buf.resizable) fail("is a view of a resizable ArrayBuffer");
      // The backing buffer is what a structured clone copies, not the view.
      if (buf.byteLength > LIMITS.bytes) fail(`holds more than ${LIMITS.bytes} bytes (counting its whole buffer)`);
      return spend(buf.byteLength);
    }
    if (typeof x !== "object") fail(`is a ${typeof x}: only plain data crosses (numbers, strings, booleans, null, lists, objects, Uint8Array)`);
    const o = x as object;
    if (seen.has(o)) fail("refers back to itself");
    seen.add(o);
    if (Array.isArray(o)) {
      const n = o.length;
      if (n > LIMITS.nodes) fail(`is a list of ${n} items (at most ${LIMITS.nodes})`);
      if (Object.keys(o).length !== n) fail("is a list with holes or extra properties: only plain lists cross");
      for (let i = 0; i < n; i++) {
        path.push(i);
        walk(o[i], depth + 1);
        path.pop();
      }
    } else {
      const proto = Object.getPrototypeOf(o);
      if (proto !== Object.prototype && proto !== null) fail(`is not a plain object (${String((o as object).constructor?.name ?? "unknown").slice(0, 40)})`);
      for (const k in o) {
        if (!Object.prototype.hasOwnProperty.call(o, k)) continue;
        if (k.length > LIMITS.key) fail(`has a key longer than ${LIMITS.key} characters`);
        spend(k.length);
        path.push(k);
        walk((o as Record<string, unknown>)[k], depth + 1);
        path.pop();
      }
    }
    seen.delete(o);
  };
  walk(v, 0);
  return v as Data;
}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const isId = (v: unknown): v is number => typeof v === "number" && Number.isSafeInteger(v) && v >= 0;
const str = (v: unknown, what: string, max = 10_000): string => {
  if (typeof v !== "string") throw new ProtocolError(`${what} must be a string`);
  if (v.length > max) throw new ProtocolError(`${what} is longer than ${max} characters`);
  return v;
};

/** Check a message a plugin sent (the host's side of the wire). */
export function decodePluginMessage(raw: unknown): PluginMessage {
  if (!isObj(raw)) throw new ProtocolError("a message must be an object");
  switch (raw.t) {
    case "ready":
      return { t: "ready" };
    case "fail":
      return { t: "fail", error: str(raw.error, "fail.error") };
    case "req": {
      if (!isId(raw.id)) throw new ProtocolError("req.id must be a whole number");
      const method = str(raw.method, "req.method", 100);
      if (!/^[a-z][A-Za-z]*(\.[a-z][A-Za-z]*)*$/.test(method)) throw new ProtocolError(`req.method "${method}" is not a method name`);
      return { t: "req", id: raw.id, method, params: checkData(raw.params ?? null, `${method} params`) };
    }
    case "done":
      if (!isId(raw.id)) throw new ProtocolError("done.id must be a whole number");
      if (raw.ok === true) return { t: "done", id: raw.id, ok: true, value: checkData(raw.value ?? null, "done.value") };
      if (raw.ok === false) return { t: "done", id: raw.id, ok: false, error: str(raw.error, "done.error") };
      throw new ProtocolError("done.ok must be true or false");
    case "pong":
      if (!isId(raw.n)) throw new ProtocolError("pong.n must be a whole number");
      return { t: "pong", n: raw.n };
    case "log": {
      const level = raw.level as LogLevel;
      if (!LEVELS.includes(level)) throw new ProtocolError(`log.level must be one of ${LEVELS.join(", ")}`);
      return { t: "log", level, text: str(raw.text, "log.text", 100_000).slice(0, 4000) };
    }
    default:
      throw new ProtocolError(`unknown message type ${JSON.stringify(raw.t)}`);
  }
}

/** Check a message the host sent (the plugin's side of the wire). */
export function decodeHostMessage(raw: unknown): HostMessage {
  if (!isObj(raw)) throw new ProtocolError("a message must be an object");
  switch (raw.t) {
    case "init": {
      if (raw.protocol !== PROTOCOL) throw new ProtocolError(`the host speaks protocol ${String(raw.protocol)}, this sandbox ${PROTOCOL}`);
      if (!isObj(raw.plugin) || !isObj(raw.files)) throw new ProtocolError("init needs plugin and files");
      const p = raw.plugin;
      const files: Record<string, string> = {};
      for (const [k, v] of Object.entries(raw.files)) files[k] = str(v, `init.files[${k}]`, LIMITS.string);
      return {
        t: "init",
        protocol: PROTOCOL,
        plugin: { id: str(p.id, "plugin.id", 100), name: str(p.name, "plugin.name", 64), version: str(p.version, "plugin.version", 64), permissions: Array.isArray(p.permissions) ? p.permissions.map((x) => str(x, "permission", 20)) : [] },
        files,
        entry: str(raw.entry, "init.entry", 200),
        units: str(raw.units, "init.units", 10),
      };
    }
    case "res":
      if (!isId(raw.id)) throw new ProtocolError("res.id must be a whole number");
      if (raw.ok === true) return { t: "res", id: raw.id, ok: true, value: checkData(raw.value ?? null, "res.value") };
      if (raw.ok === false) {
        const m: HostMessage = { t: "res", id: raw.id, ok: false, error: str(raw.error, "res.error") };
        if (typeof raw.code === "string") m.code = raw.code;
        return m;
      }
      throw new ProtocolError("res.ok must be true or false");
    case "call":
      if (!isId(raw.id)) throw new ProtocolError("call.id must be a whole number");
      if (!Array.isArray(raw.args)) throw new ProtocolError("call.args must be a list");
      return { t: "call", id: raw.id, fn: str(raw.fn, "call.fn", 100), args: raw.args.map((a, i) => checkData(a, `call.args[${i}]`)) };
    case "evt":
      return { t: "evt", name: str(raw.name, "evt.name", 100), payload: checkData(raw.payload ?? null, "evt.payload") };
    case "ping":
      if (!isId(raw.n)) throw new ProtocolError("ping.n must be a whole number");
      return { t: "ping", n: raw.n };
    default:
      throw new ProtocolError(`unknown message type ${JSON.stringify(raw.t)}`);
  }
}

// ---- frames (lane 16): what actually crosses ------------------------------------------------

const LEVEL_SET = new Set<string>(LEVELS);

/** A plugin message as one transferable frame (the sandbox's side). Throws WireError
 * ("invalid" / "too_large") when it cannot be sent; nothing is sent then. */
export function encodePluginMessage(msg: PluginMessage): ArrayBuffer {
  switch (msg.t) {
    case "ready":
      return encodeFrame("ready", 0, "", null, "ready");
    case "fail":
      return encodeFrame("fail", 0, "", msg.error.slice(0, 10_000), "fail.error");
    case "req":
      return encodeFrame("req", msg.id, msg.method, msg.params, `${msg.method} params`);
    case "done":
      return encodeFrame("done", msg.id, "", msg.ok ? { ok: true, value: msg.value } : { ok: false, error: msg.error.slice(0, 10_000) }, "result");
    case "pong":
      return encodeFrame("pong", msg.n, "", null, "pong");
    case "log":
      return encodeFrame("log", 0, msg.level, msg.text.slice(0, 4000), "log.text");
  }
}

/** A host message as one transferable frame (the app's side). */
export function encodeHostMessage(msg: HostMessage): ArrayBuffer {
  switch (msg.t) {
    case "init":
      return encodeFrame("init", 0, "", { protocol: msg.protocol, plugin: msg.plugin as unknown as Data, files: msg.files, entry: msg.entry, units: msg.units }, "init");
    case "res":
      return encodeFrame("res", msg.id, msg.method ?? "", msg.ok ? { ok: true, value: msg.value } : { ok: false, error: msg.error, code: msg.code ?? null }, `${msg.method ?? "res"} answer`);
    case "call":
      return encodeFrame("call", msg.id, msg.fn, msg.args, `${msg.fn} arguments`);
    case "evt":
      return encodeFrame("evt", 0, msg.name, msg.payload, `${msg.name} event`);
    case "ping":
      return encodeFrame("ping", msg.n, "", null, "ping");
  }
}

/** The header of a frame, or null when even that cannot be read (for answering a refusal). */
export function peekFrame(raw: unknown): FrameHeader | null {
  try {
    return readHeader(raw);
  } catch (e) {
    // Over budget: the header itself was fine, so read it again without the size check.
    if (e instanceof WireError && e.code === "too_large" && raw instanceof ArrayBuffer && raw.byteLength >= 15) {
      const b = new Uint8Array(raw, 0, Math.min(raw.byteLength, 114));
      const kinds: Record<number, FrameKind> = { 1: "ready", 2: "fail", 3: "req", 4: "done", 5: "pong", 6: "log", 11: "init", 12: "res", 13: "call", 14: "evt", 15: "ping" };
      const kind = kinds[b[4]];
      const id = new DataView(raw).getFloat64(5, true);
      const n = b[13];
      if (!kind || !Number.isSafeInteger(id) || id < 0 || 14 + n > b.length) return null;
      let name = "";
      for (let i = 0; i < n; i++) name += String.fromCharCode(b[14 + i] & 0x7f);
      return { kind, id, name, size: raw.byteLength };
    }
    return null;
  }
}

const bodyObj = (v: unknown, what: string): Record<string, unknown> => {
  if (!isObj(v)) throw new ProtocolError(`${what} must be an object`);
  return v;
};

/** Read a frame a plugin sent (the host's side of the wire). */
export function decodePluginFrame(raw: unknown): PluginMessage {
  const { header: h, body } = decodeFrame(raw);
  switch (h.kind) {
    case "ready":
      return { t: "ready" };
    case "fail":
      return { t: "fail", error: str(body, "fail.error") };
    case "req": {
      if (!/^[a-z][A-Za-z]*(\.[a-z][A-Za-z]*)*$/.test(h.name)) throw new ProtocolError(`req.method "${h.name}" is not a method name`);
      return { t: "req", id: h.id, method: h.name, params: body as Data };
    }
    case "done": {
      const b = bodyObj(body, "done");
      if (b.ok === true) return { t: "done", id: h.id, ok: true, value: (b.value ?? null) as Data };
      if (b.ok === false) return { t: "done", id: h.id, ok: false, error: str(b.error, "done.error") };
      throw new ProtocolError("done.ok must be true or false");
    }
    case "pong":
      return { t: "pong", n: h.id };
    case "log": {
      if (!LEVEL_SET.has(h.name)) throw new ProtocolError(`log.level must be one of ${LEVELS.join(", ")}`);
      return { t: "log", level: h.name as LogLevel, text: str(body, "log.text", 100_000).slice(0, 4000) };
    }
    default:
      throw new ProtocolError(`a plugin cannot send a ${h.kind} message`);
  }
}

/** Read a frame the host sent (the plugin's side of the wire). */
export function decodeHostFrame(raw: unknown): HostMessage {
  const { header: h, body } = decodeFrame(raw);
  switch (h.kind) {
    case "init": {
      const b = bodyObj(body, "init");
      return decodeHostMessage({ t: "init", ...b });
    }
    case "res": {
      const b = bodyObj(body, "res");
      if (b.ok === true) return { t: "res", id: h.id, ok: true, value: (b.value ?? null) as Data, method: h.name };
      if (b.ok === false) {
        const m: HostMessage = { t: "res", id: h.id, ok: false, error: str(b.error, "res.error"), method: h.name };
        if (typeof b.code === "string") m.code = b.code.slice(0, 40);
        return m;
      }
      throw new ProtocolError("res.ok must be true or false");
    }
    case "call":
      if (!Array.isArray(body)) throw new ProtocolError("call.args must be a list");
      return { t: "call", id: h.id, fn: h.name, args: body as Data[] };
    case "evt":
      return { t: "evt", name: h.name, payload: (body ?? null) as Data };
    case "ping":
      return { t: "ping", n: h.id };
    default:
      throw new ProtocolError(`the app cannot send a ${h.kind} message`);
  }
}

/** Byte arrays to transfer with a message (the receiver takes ownership; no copy). */
export function transferables(v: unknown, out: ArrayBuffer[] = []): ArrayBuffer[] {
  if (v instanceof Uint8Array) {
    if (v.buffer instanceof ArrayBuffer && v.byteOffset === 0 && v.byteLength === v.buffer.byteLength && !out.includes(v.buffer)) out.push(v.buffer);
  } else if (Array.isArray(v)) v.forEach((x) => transferables(x, out));
  else if (isObj(v)) Object.values(v).forEach((x) => transferables(x, out));
  return out;
}

const stringify = JSON.stringify;

/** One line of text from console-style arguments (what a plugin logs). Uses no Array
 * methods and the JSON.stringify of when this module loaded, so a plugin that replaces
 * built-ins still logs (lane 16). */
export function logText(args: unknown[]): string {
  let out = "";
  for (let i = 0; i < args.length && out.length < 4000; i++) {
    const a = args[i];
    let t: string;
    if (typeof a === "string") t = a;
    else if (a instanceof Error) t = `${a.name}: ${a.message}`;
    else
      try {
        t = String(stringify(a, (_k, v) => (v instanceof Uint8Array ? `<${v.byteLength} bytes>` : v)));
      } catch {
        try {
          t = String(a);
        } catch {
          t = "?";
        }
      }
    out += (i ? " " : "") + t;
  }
  return out.slice(0, 4000);
}
