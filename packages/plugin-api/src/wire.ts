// The wire format between the app and a plugin's Web Worker (lane 16): every message is
// serialized ONCE, by the side that sends it, into one ArrayBuffer that is transferred
// (zero-copy), never structured-cloned. The receiver checks the buffer's byteLength against
// the budget for that kind of message before it reads a single value, and then reads the
// values out of the bytes with every limit applied as it goes, before anything is allocated
// for them. So a hostile sender cannot make the receiver hold more than one budget's worth of
// bytes, walk a sparse array of length 2^32, read a getter twice, or allocate a 1.5 GB buffer
// behind a 1-byte view: none of those survive serialization, and the receiver never trusts
// the sender's own checks (a plugin can patch every built-in it can see).
//
//   frame   = magic "SHW1" | kind (u8) | id (f64) | name length (u8) | name (ASCII) | body
//   body    = one value
//   value   = 0x00 null | 0x01 false | 0x02 true | 0x03 f64 | 0x04 i32
//           | 0x05 string (u32 byte length, UTF-8) | 0x06 bytes (u32 length, raw)
//           | 0x07 list (u32 count, values) | 0x08 object (u32 count, (u32 key length, UTF-8 key, value)*)
//
// All numbers little-endian. `name` is the request's method, the callback's name, the event's
// name or the log level, so a receiver knows the budget (and whom to answer) from the header
// alone.

/** Limits on values (the same on both sides of the wire). */
export const WIRE_LIMITS = {
  /** Characters in one string. */
  string: 4 * 1024 * 1024,
  /** Bytes in one byte array. */
  bytes: 32 * 1024 * 1024,
  /** Values in one message (each list, object, string, number ... counts one). */
  nodes: 1_000_000,
  /** Lists and objects inside each other. */
  depth: 64,
  /** Characters in one object key. */
  key: 256,
} as const;

const MiB = 1024 * 1024;

/** The largest frame of any kind; a buffer over this is refused before its header is read. */
export const MAX_FRAME = 34 * MiB;

/** Total bytes one message may take on the wire, by kind (and by method for the big ones). */
export const FRAME_BUDGET = {
  /** Every request and answer without its own budget. */
  default: 4 * MiB,
  /** file.save (the file's bytes, up to 32 MiB) and the answer to file.open. */
  file: 34 * MiB,
  /** net.fetch: a request body or an answer (up to 10 MiB). */
  net: 12 * MiB,
  /** The modules a plugin is started with (init). */
  init: 24 * MiB,
  /** Log lines, failures, pongs, pings, ready: small by nature. */
  small: 64 * 1024,
} as const;

export type FrameKind = "ready" | "fail" | "req" | "done" | "pong" | "log" | "init" | "res" | "call" | "evt" | "ping";

const KIND_CODE: Record<FrameKind, number> = { ready: 1, fail: 2, req: 3, done: 4, pong: 5, log: 6, init: 11, res: 12, call: 13, evt: 14, ping: 15 };
const CODE_KIND: Record<number, FrameKind> = Object.fromEntries(Object.entries(KIND_CODE).map(([k, v]) => [v, k as FrameKind]));

/** The budget for a frame, from its header. */
export function frameBudget(kind: FrameKind, name: string): number {
  switch (kind) {
    case "req":
    case "res":
      if (name === "file.save" || name === "file.open") return FRAME_BUDGET.file;
      if (name === "net.fetch") return FRAME_BUDGET.net;
      return FRAME_BUDGET.default;
    case "init":
      return FRAME_BUDGET.init;
    case "done":
    case "call":
    case "evt":
      return FRAME_BUDGET.default;
    default:
      return FRAME_BUDGET.small;
  }
}

export class ProtocolError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ProtocolError";
  }
}

/** Why a value or a frame could not cross: "invalid" (not plain data) or "too_large" (over a limit). */
export class WireError extends ProtocolError {
  readonly code: "invalid" | "too_large";
  constructor(message: string, code: "invalid" | "too_large") {
    super(message);
    this.code = code;
  }
}

// ---- intrinsics, taken when this module loads (before any plugin code runs in a sandbox) ----

const isArray = Array.isArray;
const objectKeys = Object.keys;
const getProto = Object.getPrototypeOf;
const ObjectProto = Object.prototype;
const isFiniteNumber = Number.isFinite;
const isInteger = Number.isInteger;
const objectIs = Object.is;
const U8 = Uint8Array;
const TypedArrayProto = getProto(Uint8Array.prototype) as object;
const descriptor = Object.getOwnPropertyDescriptor;
const taBuffer = descriptor(TypedArrayProto, "buffer")!.get!;
const taOffset = descriptor(TypedArrayProto, "byteOffset")!.get!;
const taLength = descriptor(TypedArrayProto, "byteLength")!.get!;
const abResizable = descriptor(ArrayBuffer.prototype, "resizable")?.get;
const abByteLength = descriptor(ArrayBuffer.prototype, "byteLength")!.get!;
const taTag = descriptor(TypedArrayProto, Symbol.toStringTag)!.get!;
const SAB = (globalThis as { SharedArrayBuffer?: unknown }).SharedArrayBuffer as (new (n: number) => object) | undefined;
const apply = Reflect.apply;
const u8Set = U8.prototype.set;
const textEncoder = new TextEncoder();
const encodeInto = TextEncoder.prototype.encodeInto;
const fatalDecoder = new TextDecoder("utf-8", { fatal: true });
const decode = TextDecoder.prototype.decode;
const instanceOf = (x: unknown, C: unknown): boolean => {
  try {
    return typeof C === "function" && x instanceof (C as new () => unknown);
  } catch {
    return false;
  }
};

/** Whether `x` is a Uint8Array (by its internal slot, not a plugin-patched prototype). */
function isU8(x: object): x is Uint8Array {
  try {
    // %TypedArray%.prototype[Symbol.toStringTag] reads the internal slot.
    return apply(taTag, x, []) === "Uint8Array";
  } catch {
    return false;
  }
}

/** Whether `buf` is a resizable or shared buffer (refused: their size can change under us). */
function oddBuffer(buf: object): string | null {
  if (SAB && instanceOf(buf, SAB)) return "a SharedArrayBuffer";
  try {
    if (abResizable && apply(abResizable, buf, [])) return "a resizable ArrayBuffer";
  } catch {
    return "not an ArrayBuffer";
  }
  return null;
}

// ---- paths, built only when something is refused ----

type PathStep = string | number;
function pathText(root: string, path: PathStep[]): string {
  let s = root;
  for (const p of path) s += typeof p === "number" ? `[${p}]` : /^[A-Za-z_$][\w$]*$/.test(p) ? `.${p}` : `[${JSON.stringify(p.length > 40 ? `${p.slice(0, 40)}…` : p)}]`;
  return s;
}

// ---- writing ----

/** A value already serialized (a batch's op, encoded when the plugin added it). */
export class Encoded {
  readonly bytes: Uint8Array;
  readonly nodes: number;
  constructor(bytes: Uint8Array, nodes: number) {
    this.bytes = bytes;
    this.nodes = nodes;
  }
}

class Writer {
  buf: Uint8Array;
  view: DataView;
  at = 0;
  nodes = 0;
  readonly cap: number;
  readonly root: string;
  readonly path: PathStep[] = [];
  constructor(cap: number, root: string, initial = 1024) {
    this.cap = cap;
    this.root = root;
    this.buf = new U8(Math.min(initial, cap));
    this.view = new DataView(this.buf.buffer);
  }
  fail(message: string, code: "invalid" | "too_large"): never {
    throw new WireError(`${pathText(this.root, this.path)} ${message}`, code);
  }
  ensure(n: number): void {
    const need = this.at + n;
    if (need <= this.buf.length) return;
    if (need > this.cap) throw new WireError(`${this.root} is too large to send (more than ${Math.round(this.cap / 1024)} KiB)`, "too_large");
    let size = this.buf.length * 2;
    while (size < need) size *= 2;
    const next = new U8(Math.min(size, this.cap));
    apply(u8Set, next, [this.buf.subarray(0, this.at)]);
    this.buf = next;
    this.view = new DataView(next.buffer);
  }
  u8(x: number): void {
    this.ensure(1);
    this.buf[this.at++] = x;
  }
  u32(x: number): void {
    this.ensure(4);
    this.view.setUint32(this.at, x, true);
    this.at += 4;
  }
  f64(x: number): void {
    this.ensure(8);
    this.view.setFloat64(this.at, x, true);
    this.at += 8;
  }
  /** A UTF-8 string with its u32 byte length first. */
  text(s: string): void {
    // Room for the worst case (3 bytes a UTF-16 unit), but never past the budget: a string
    // that does not fit in what is left is refused when it does not encode whole.
    const room = this.cap - this.at - 4;
    if (room < 0) throw new WireError(`${this.root} is too large to send (more than ${Math.round(this.cap / 1024)} KiB)`, "too_large");
    const want = Math.min(s.length * 3, room);
    this.ensure(4 + want);
    const lenAt = this.at;
    this.at += 4;
    const r = apply(encodeInto, textEncoder, [s, this.buf.subarray(this.at, this.at + want)]) as { read: number; written: number };
    if (r.read < s.length) throw new WireError(`${this.root} is too large to send (more than ${Math.round(this.cap / 1024)} KiB)`, "too_large");
    this.view.setUint32(lenAt, r.written, true);
    this.at += r.written;
  }
  raw(bytes: Uint8Array): void {
    this.ensure(bytes.length);
    apply(u8Set, this.buf, [bytes, this.at]);
    this.at += bytes.length;
  }
  value(x: unknown, depth: number): void {
    if (++this.nodes > WIRE_LIMITS.nodes) this.fail(`is too large (more than ${WIRE_LIMITS.nodes} values)`, "too_large");
    if (x === null || x === undefined) return this.u8(0);
    if (x === false) return this.u8(1);
    if (x === true) return this.u8(2);
    if (typeof x === "number") {
      if (!isFiniteNumber(x)) this.fail(`is not a finite number (${x})`, "invalid");
      if (isInteger(x) && x >= -2147483648 && x <= 2147483647 && !objectIs(x, -0)) {
        this.u8(4);
        this.ensure(4);
        this.view.setInt32(this.at, x, true);
        this.at += 4;
        return;
      }
      this.u8(3);
      return this.f64(x);
    }
    if (typeof x === "string") {
      if (x.length > WIRE_LIMITS.string) this.fail(`is a string longer than ${WIRE_LIMITS.string} characters`, "too_large");
      this.u8(5);
      return this.text(x);
    }
    if (typeof x !== "object") this.fail(`is a ${typeof x}: only plain data crosses (numbers, strings, booleans, null, lists, objects, Uint8Array)`, "invalid");
    if (x instanceof Encoded) {
      this.nodes += x.nodes - 1;
      if (this.nodes > WIRE_LIMITS.nodes) this.fail(`is too large (more than ${WIRE_LIMITS.nodes} values)`, "too_large");
      return this.raw(x.bytes);
    }
    if (depth >= WIRE_LIMITS.depth) this.fail(`is nested more than ${WIRE_LIMITS.depth} deep`, "invalid");
    const o = x as object;
    if (isU8(o)) {
      const buffer = apply(taBuffer, o, []) as object;
      const odd = oddBuffer(buffer);
      if (odd) this.fail(`is a view of ${odd}: copy the bytes into a plain Uint8Array first`, "invalid");
      const offset = apply(taOffset, o, []) as number;
      const length = apply(taLength, o, []) as number;
      if (length > WIRE_LIMITS.bytes) this.fail(`holds more than ${WIRE_LIMITS.bytes} bytes`, "too_large");
      this.u8(6);
      this.u32(length);
      // Only the view's own bytes go out, never the rest of its buffer.
      return this.raw(new U8(buffer as ArrayBuffer, offset, length));
    }
    if (isArray(o)) {
      const list = o as unknown[];
      const n = list.length;
      if (!(n >= 0 && n <= WIRE_LIMITS.nodes)) this.fail(`is a list of ${n} items (at most ${WIRE_LIMITS.nodes})`, "too_large");
      // Holes and extra named properties: a list must be exactly its items.
      if (objectKeys(list).length !== n) this.fail("is a list with holes or extra properties: only plain lists cross", "invalid");
      this.u8(7);
      this.u32(n);
      for (let i = 0; i < n; i++) {
        this.path.push(i);
        this.value(list[i], depth + 1);
        this.path.pop();
      }
      return;
    }
    const proto = getProto(o);
    if (proto !== ObjectProto && proto !== null) {
      let what = "unknown";
      try {
        what = String((o as { constructor?: { name?: unknown } }).constructor?.name ?? "unknown").slice(0, 40);
      } catch {
        /* a throwing getter */
      }
      this.fail(`is not a plain object (${what})`, "invalid");
    }
    const keys = objectKeys(o);
    if (keys.length > WIRE_LIMITS.nodes) this.fail(`has more than ${WIRE_LIMITS.nodes} fields`, "too_large");
    this.u8(8);
    const countAt = this.at;
    this.u32(0);
    let count = 0;
    for (let i = 0; i < keys.length; i++) {
      const k = keys[i];
      if (k.length > WIRE_LIMITS.key) this.fail(`has a key longer than ${WIRE_LIMITS.key} characters`, "too_large");
      if (k === "__proto__") this.fail('has a "__proto__" key', "invalid");
      const v = (o as Record<string, unknown>)[k];
      // undefined fields are left out, as JSON would.
      if (v === undefined) continue;
      this.text(k);
      this.path.push(k);
      this.value(v, depth + 1);
      this.path.pop();
      count++;
    }
    this.view.setUint32(countAt, count, true);
  }
  frame(kind: FrameKind, id: number, name: string): void {
    this.ensure(14 + name.length);
    this.buf[0] = 0x53;
    this.buf[1] = 0x48;
    this.buf[2] = 0x57;
    this.buf[3] = 0x31;
    this.buf[4] = KIND_CODE[kind];
    this.view.setFloat64(5, id, true);
    if (name.length > 100 || !/^[\x20-\x7e]*$/.test(name)) throw new WireError(`a ${kind} name must be up to 100 plain characters`, "invalid");
    this.buf[13] = name.length;
    for (let i = 0; i < name.length; i++) this.buf[14 + i] = name.charCodeAt(i);
    this.at = 14 + name.length;
  }
  done(): ArrayBuffer {
    // A fresh buffer of exactly the frame's size: what is transferred is what is measured.
    return this.buf.slice(0, this.at).buffer;
  }
}

/** Serialize one value on its own (a batch op), with the value limits. */
export function encodeValue(v: unknown, root = "value", cap: number = FRAME_BUDGET.default): Encoded {
  const w = new Writer(cap, root, 256);
  w.value(v, 0);
  return new Encoded(w.buf.slice(0, w.at), w.nodes);
}

/** Serialize a message: header and body, within `cap` bytes (default: the kind's budget). */
export function encodeFrame(kind: FrameKind, id: number, name: string, body: unknown, root: string, cap = frameBudget(kind, name)): ArrayBuffer {
  const w = new Writer(cap, root);
  w.frame(kind, id, name);
  w.value(body, 0);
  return w.done();
}

// ---- reading ----

export interface FrameHeader {
  kind: FrameKind;
  id: number;
  name: string;
  /** Total bytes of the frame. */
  size: number;
}

/**
 * Read a frame's header, checking the buffer before anything else: it must be a plain
 * (not shared, not resizable) ArrayBuffer within MAX_FRAME and within its kind's budget.
 */
export function readHeader(raw: unknown): FrameHeader {
  if (!instanceOf(raw, ArrayBuffer)) throw new WireError("a message must be one ArrayBuffer (the serialized frame)", "invalid");
  const odd = oddBuffer(raw as object);
  if (odd) throw new WireError(`a message cannot be ${odd}`, "invalid");
  const size = apply(abByteLength, raw, []) as number;
  if (size > MAX_FRAME) throw new WireError(`a message of ${size} bytes is over the ${MAX_FRAME / MiB} MiB any message may take`, "too_large");
  if (size < 15) throw new WireError("a message is too short to be a frame", "invalid");
  const b = new U8(raw as ArrayBuffer);
  if (b[0] !== 0x53 || b[1] !== 0x48 || b[2] !== 0x57 || b[3] !== 0x31) throw new WireError("not a Sawhorse plugin frame", "invalid");
  const kind = CODE_KIND[b[4]];
  if (!kind) throw new WireError(`unknown message kind ${b[4]}`, "invalid");
  const id = new DataView(raw as ArrayBuffer).getFloat64(5, true);
  if (!(Number.isSafeInteger(id) && id >= 0)) throw new WireError("a message id must be a whole number", "invalid");
  const nameLen = b[13];
  if (nameLen > 100 || 14 + nameLen > size) throw new WireError("a message name is too long", "invalid");
  let name = "";
  for (let i = 0; i < nameLen; i++) {
    const c = b[14 + i];
    if (c < 0x20 || c > 0x7e) throw new WireError("a message name must be plain characters", "invalid");
    name += String.fromCharCode(c);
  }
  const budget = frameBudget(kind, name);
  if (size > budget) throw new WireError(`a ${kind}${name ? ` ${name}` : ""} message of ${Math.round(size / 1024)} KiB is over its ${Math.round(budget / 1024)} KiB budget`, "too_large");
  return { kind, id, name, size };
}

class Reader {
  readonly b: Uint8Array;
  readonly view: DataView;
  at: number;
  nodes = 0;
  readonly root: string;
  readonly path: PathStep[] = [];
  constructor(buf: ArrayBuffer, at: number, root: string) {
    this.b = new U8(buf);
    this.view = new DataView(buf);
    this.at = at;
    this.root = root;
  }
  fail(message: string, code: "invalid" | "too_large" = "invalid"): never {
    throw new WireError(`${pathText(this.root, this.path)} ${message}`, code);
  }
  left(): number {
    return this.b.length - this.at;
  }
  need(n: number): void {
    if (n > this.left()) this.fail("runs past the end of the message");
  }
  u32(): number {
    this.need(4);
    const x = this.view.getUint32(this.at, true);
    this.at += 4;
    return x;
  }
  text(maxChars: number, what: string): string {
    const n = this.u32();
    // Refused on the byte count before decoding (UTF-8 takes at most 3 bytes a character
    // here: 4-byte characters are two UTF-16 units).
    if (n > maxChars * 3) this.fail(`${what} is longer than ${maxChars} characters`, "too_large");
    this.need(n);
    let s: string;
    try {
      s = apply(decode, fatalDecoder, [this.b.subarray(this.at, this.at + n)]) as string;
    } catch {
      this.fail(`${what} is not UTF-8 text`);
    }
    this.at += n;
    if (s.length > maxChars) this.fail(`${what} is longer than ${maxChars} characters`, "too_large");
    return s;
  }
  value(depth: number): unknown {
    if (++this.nodes > WIRE_LIMITS.nodes) this.fail(`is too large (more than ${WIRE_LIMITS.nodes} values)`, "too_large");
    this.need(1);
    const tag = this.b[this.at++];
    switch (tag) {
      case 0:
        return null;
      case 1:
        return false;
      case 2:
        return true;
      case 3: {
        this.need(8);
        const x = this.view.getFloat64(this.at, true);
        this.at += 8;
        if (!Number.isFinite(x)) this.fail(`is not a finite number (${x})`);
        return x;
      }
      case 4: {
        this.need(4);
        const x = this.view.getInt32(this.at, true);
        this.at += 4;
        return x;
      }
      case 5:
        return this.text(WIRE_LIMITS.string, "is a string that");
      case 6: {
        const n = this.u32();
        if (n > WIRE_LIMITS.bytes) this.fail(`holds more than ${WIRE_LIMITS.bytes} bytes`, "too_large");
        this.need(n);
        // A copy with its own buffer: nothing downstream keeps the whole frame alive.
        const out = this.b.slice(this.at, this.at + n);
        this.at += n;
        return out;
      }
      case 7: {
        if (depth >= WIRE_LIMITS.depth) this.fail(`is nested more than ${WIRE_LIMITS.depth} deep`);
        const n = this.u32();
        // Every item takes at least one byte: a count the bytes cannot hold is refused before
        // anything is allocated for it.
        if (n > this.left() || n > WIRE_LIMITS.nodes) this.fail(`is a list of ${n} items, more than the message holds`, "too_large");
        const out: unknown[] = new Array(n);
        for (let i = 0; i < n; i++) {
          this.path.push(i);
          out[i] = this.value(depth + 1);
          this.path.pop();
        }
        return out;
      }
      case 8: {
        if (depth >= WIRE_LIMITS.depth) this.fail(`is nested more than ${WIRE_LIMITS.depth} deep`);
        const n = this.u32();
        // Every field takes at least 5 bytes (a key's length and a value's tag).
        if (n * 5 > this.left() || n > WIRE_LIMITS.nodes) this.fail(`is an object of ${n} fields, more than the message holds`, "too_large");
        const out: Record<string, unknown> = {};
        for (let i = 0; i < n; i++) {
          const k = this.text(WIRE_LIMITS.key, "has a key that");
          if (k === "__proto__") this.fail('has a "__proto__" key');
          if (Object.prototype.hasOwnProperty.call(out, k)) this.fail(`has the key ${JSON.stringify(k.slice(0, 40))} twice`);
          this.path.push(k);
          out[k] = this.value(depth + 1);
          this.path.pop();
        }
        return out;
      }
      default:
        return this.fail(`holds an unknown value tag ${tag}`);
    }
  }
}

/** Read a whole frame: the header (checked first) and its body, every limit applied while reading. */
export function decodeFrame(raw: unknown, root?: string): { header: FrameHeader; body: unknown } {
  const header = readHeader(raw);
  const r = new Reader(raw as ArrayBuffer, 14 + header.name.length, root ?? `${header.kind}${header.name ? ` ${header.name}` : ""}`);
  const body = r.value(0);
  if (r.at !== header.size) r.fail("has bytes after its value");
  return { header, body };
}

/** Read one value written by encodeValue (tests and tools). */
export function decodeValue(bytes: Uint8Array, root = "value"): unknown {
  const copy = bytes.slice().buffer;
  const r = new Reader(copy, 0, root);
  const v = r.value(0);
  if (r.at !== copy.byteLength) r.fail("has bytes after its value");
  return v;
}
