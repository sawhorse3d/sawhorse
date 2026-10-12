// The wire protocol between the app (the host) and one plugin's Web Worker. Every message
// is plain data: JSON values plus Uint8Array, checked on the way in by the side that
// receives it. Nothing else crosses: no functions, no DOM objects, no class instances.
//
//   host -> plugin   init, res (an answer to a req), call (run a callback: a command, a
//                    panel event, a tool event; answered by done), evt (an event the plugin
//                    subscribed to), ping (the watchdog)
//   plugin -> host   ready / fail (after the entry ran), req (an API call, answered by res),
//                    done, pong, log
//
// Requests carry ids so answers can arrive in any order. The host enforces permissions on
// every req; the plugin side only shapes calls.

export const PROTOCOL = 1;

/** Largest string, byte array and message the codec accepts. */
export const LIMITS = { string: 4 * 1024 * 1024, bytes: 32 * 1024 * 1024, nodes: 1_000_000, depth: 64 };

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
  | { t: "res"; id: number; ok: true; value: Data }
  | { t: "res"; id: number; ok: false; error: string; code?: string }
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

export class ProtocolError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ProtocolError";
  }
}

const LEVELS: LogLevel[] = ["debug", "log", "info", "warn", "error"];

/**
 * Check that `v` is plain data (JSON values, finite numbers, Uint8Array), within the size
 * limits. Throws a ProtocolError naming where the first problem is.
 */
export function checkData(v: unknown, where = "value"): Data {
  let nodes = 0;
  const walk = (x: unknown, at: string, depth: number, seen: Set<object>): void => {
    if (++nodes > LIMITS.nodes) throw new ProtocolError(`${where} is too large (more than ${LIMITS.nodes} values)`);
    if (depth > LIMITS.depth) throw new ProtocolError(`${at} is nested more than ${LIMITS.depth} deep`);
    if (x === null || typeof x === "boolean") return;
    if (typeof x === "number") {
      if (!Number.isFinite(x)) throw new ProtocolError(`${at} is not a finite number (${x})`);
      return;
    }
    if (typeof x === "string") {
      if (x.length > LIMITS.string) throw new ProtocolError(`${at} is a string longer than ${LIMITS.string} characters`);
      return;
    }
    if (x instanceof Uint8Array) {
      if (x.byteLength > LIMITS.bytes) throw new ProtocolError(`${at} holds more than ${LIMITS.bytes} bytes`);
      return;
    }
    if (typeof x !== "object") throw new ProtocolError(`${at} is a ${typeof x}: only plain data crosses (numbers, strings, booleans, null, lists, objects, Uint8Array)`);
    if (seen.has(x)) throw new ProtocolError(`${at} refers back to itself`);
    seen.add(x);
    if (Array.isArray(x)) {
      x.forEach((y, i) => walk(y, `${at}[${i}]`, depth + 1, seen));
    } else {
      const proto = Object.getPrototypeOf(x);
      if (proto !== Object.prototype && proto !== null) throw new ProtocolError(`${at} is not a plain object (${(x as object).constructor?.name ?? "unknown"})`);
      for (const [k, y] of Object.entries(x)) walk(y, `${at}.${k}`, depth + 1, seen);
    }
    seen.delete(x);
  };
  walk(v, where, 0, new Set());
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

/** Byte arrays to transfer with a message (the receiver takes ownership; no copy). */
export function transferables(v: unknown, out: ArrayBuffer[] = []): ArrayBuffer[] {
  if (v instanceof Uint8Array) {
    if (v.buffer instanceof ArrayBuffer && v.byteOffset === 0 && v.byteLength === v.buffer.byteLength && !out.includes(v.buffer)) out.push(v.buffer);
  } else if (Array.isArray(v)) v.forEach((x) => transferables(x, out));
  else if (isObj(v)) Object.values(v).forEach((x) => transferables(x, out));
  return out;
}

/** One line of text from console-style arguments (what a plugin logs). */
export function logText(args: unknown[]): string {
  return args
    .map((a) => {
      if (typeof a === "string") return a;
      if (a instanceof Error) return `${a.name}: ${a.message}`;
      try {
        return JSON.stringify(a, (_k, v) => (v instanceof Uint8Array ? `<${v.byteLength} bytes>` : v));
      } catch {
        return String(a);
      }
    })
    .join(" ")
    .slice(0, 4000);
}
