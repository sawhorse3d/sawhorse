// Lane 16: the serialize-once wire format. Every limit is enforced by the side that reads a
// frame, from the bytes, before anything is allocated for what the bytes claim; the sender's
// own checks are a courtesy.
import { describe, expect, it } from "vitest";
import { decodeHostFrame, decodePluginFrame, encodeHostMessage, encodePluginMessage, peekFrame, type PluginMessage } from "../src/rpc.ts";
import { decodeFrame, decodeValue, encodeFrame, encodeValue, FRAME_BUDGET, MAX_FRAME, readHeader, WIRE_LIMITS, WireError } from "../src/wire.ts";

/** A frame header by hand: kind code, id, name; the body bytes follow. */
function forge(kind: number, id: number, name: string, body: number[] | Uint8Array, size?: number): ArrayBuffer {
  const head = 14 + name.length;
  const total = size ?? head + body.length;
  const buf = new ArrayBuffer(total);
  const b = new Uint8Array(buf);
  b.set([0x53, 0x48, 0x57, 0x31, kind]);
  new DataView(buf).setFloat64(5, id, true);
  b[13] = name.length;
  for (let i = 0; i < name.length; i++) b[14 + i] = name.charCodeAt(i);
  b.set(body instanceof Uint8Array ? body : Uint8Array.from(body), head);
  return buf;
}
const u32 = (n: number) => [n & 255, (n >>> 8) & 255, (n >>> 16) & 255, (n >>> 24) & 255];

describe("values", () => {
  it("round-trip plain data exactly (numbers, text, bytes, lists, objects)", () => {
    const v = { a: [1, -2, 2.5, 2 ** 40, -0.5, 0], s: "héllo ✓ 🙂", n: null, t: true, f: false, bytes: new Uint8Array([0, 1, 255]), nested: { deep: [[{ x: 1 }]] }, constructor: "a key like any other" };
    const back = decodeValue(encodeValue(v).bytes) as typeof v;
    expect(back).toEqual(v);
    expect(back.bytes).toBeInstanceOf(Uint8Array);
    expect(Object.getPrototypeOf(back)).toBe(Object.prototype);
  });

  it("leave out undefined fields and turn undefined items into null, as JSON would", () => {
    expect(decodeValue(encodeValue({ a: undefined, b: [undefined, 1] }).bytes)).toEqual({ b: [null, 1] });
  });

  it.each([
    [{ f: () => 1 }, "value.f is a function", "invalid"],
    [{ n: NaN }, "value.n is not a finite number", "invalid"],
    [{ d: new Date() }, "value.d is not a plain object (Date)", "invalid"],
    [[1, [2, { big: 10n }]], "value[1][1].big is a bigint", "invalid"],
    [{ ["k".repeat(257)]: 1 }, "has a key longer than 256 characters", "too_large"],
    [JSON.parse('{"__proto__": 1}'), 'has a "__proto__" key', "invalid"],
  ])("refuse %#: %s", (v, msg, code) => {
    let err: unknown;
    try {
      encodeValue(v);
    } catch (e) {
      err = e;
    }
    expect(err).toBeInstanceOf(WireError);
    expect((err as WireError).message).toContain(msg);
    expect((err as WireError).code).toBe(code);
  });

  it("refuse a sparse list of length 2^32-1 at once, without walking its holes", () => {
    const a: unknown[] = [];
    a.length = 2 ** 32 - 1;
    a[0] = 1;
    const t0 = performance.now();
    expect(() => encodeValue({ a })).toThrow(/a list of 4294967295 items/);
    const holey = [1, , 3];
    expect(() => encodeValue(holey)).toThrow(/holes or extra properties/);
    const extra = Object.assign([1, 2], { sneaky: "x".repeat(10) });
    expect(() => encodeValue(extra)).toThrow(/holes or extra properties/);
    expect(performance.now() - t0).toBeLessThan(100);
  });

  it("send only a view's own bytes, never the rest of its buffer (a 1-byte view of 64 MiB is 1 byte)", () => {
    const view = new Uint8Array(new ArrayBuffer(64 * 1024 * 1024), 1000, 1);
    view[0] = 42;
    const e = encodeValue({ v: view });
    expect(e.bytes.length).toBeLessThan(32);
    const back = decodeValue(e.bytes) as { v: Uint8Array };
    expect([...back.v]).toEqual([42]);
    expect(back.v.buffer.byteLength).toBe(1);
  });

  it("refuse views of shared or resizable buffers", () => {
    if (typeof SharedArrayBuffer !== "undefined") expect(() => encodeValue(new Uint8Array(new SharedArrayBuffer(4)))).toThrow(/SharedArrayBuffer/);
    const R = ArrayBuffer as unknown as new (n: number, o: { maxByteLength: number }) => ArrayBuffer;
    const rb = new R(4, { maxByteLength: 1024 });
    if ((rb as { resizable?: boolean }).resizable) expect(() => encodeValue(new Uint8Array(rb))).toThrow(/resizable/);
  });

  it("read each getter once: what is checked is what is sent", () => {
    let reads = 0;
    const o = {
      get x() {
        reads++;
        return reads === 1 ? "small" : "x".repeat(5_000_000);
      },
    };
    expect(decodeValue(encodeValue(o).bytes)).toEqual({ x: "small" });
    expect(reads).toBe(1);
  });

  it("are not fooled by patched Array.prototype or Object methods", () => {
    const saved = { forEach: Array.prototype.forEach, map: Array.prototype.map, entries: Object.entries, keys: Object.keys, isArray: Array.isArray };
    try {
      Array.prototype.forEach = () => undefined;
      Array.prototype.map = () => [];
      Object.entries = () => [];
      Object.keys = () => [];
      Array.isArray = ((_x: unknown) => false) as unknown as typeof Array.isArray;
      const back = decodeValue(encodeValue({ list: [1, 2, 3], o: { a: 1 } }).bytes);
      Object.assign(Object, { keys: saved.keys, entries: saved.entries });
      Array.isArray = saved.isArray;
      expect(back).toEqual({ list: [1, 2, 3], o: { a: 1 } });
    } finally {
      Array.prototype.forEach = saved.forEach;
      Array.prototype.map = saved.map;
      Object.entries = saved.entries;
      Object.keys = saved.keys;
      Array.isArray = saved.isArray;
    }
  });

  it("stop a string at the budget, not at three bytes a character", () => {
    // 1.5 M characters is 1.5 MiB of UTF-8: well under a 4 MiB frame.
    expect(() => encodeFrame("req", 1, "storage.set", { v: "a".repeat(1_500_000) }, "p")).not.toThrow();
    let err: WireError | null = null;
    try {
      encodeFrame("req", 1, "storage.set", { v: "a".repeat(4_000_000), w: "b".repeat(1_000_000) }, "p");
    } catch (e) {
      err = e as WireError;
    }
    expect(err?.code).toBe("too_large");
  });
});

describe("frames", () => {
  it("round-trip every message kind both ways", () => {
    const plugin: PluginMessage[] = [
      { t: "ready" },
      { t: "fail", error: "boom" },
      { t: "req", id: 4, method: "model.batch", params: { name: "x", ops: [{ op: "rename", item: { object: 1 }, name: "A" }] } },
      { t: "done", id: 9, ok: true, value: [1, 2] },
      { t: "done", id: 9, ok: false, error: "nope" },
      { t: "pong", n: 3 },
      { t: "log", level: "warn", text: "careful" },
    ];
    for (const m of plugin) expect(decodePluginFrame(encodePluginMessage(m))).toEqual(m);
    const host = [
      { t: "init", protocol: 1, plugin: { id: "org.example.a", name: "A", version: "1.0.0", permissions: ["ui"] }, files: { "main.js": "export {}" }, entry: "main.js", units: "mm" },
      { t: "res", id: 1, ok: true, value: { solids: 3 }, method: "model.stats" },
      { t: "res", id: 1, ok: false, error: "not allowed", code: "permission", method: "model.batch" },
      { t: "call", id: 2, fn: "command", args: ["generate"] },
      { t: "evt", name: "documentChanged", payload: { hash: "abc" } },
      { t: "ping", n: 7 },
    ] as const;
    for (const m of host) expect(decodeHostFrame(encodeHostMessage(m as never))).toEqual(m);
  });

  it("check the buffer before reading it: not an ArrayBuffer, shared, over any budget, over its kind's budget", () => {
    expect(() => readHeader({ t: "req" })).toThrow(/one ArrayBuffer/);
    expect(() => readHeader(new Uint8Array(20))).toThrow(/one ArrayBuffer/);
    if (typeof SharedArrayBuffer !== "undefined") expect(() => readHeader(new SharedArrayBuffer(64))).toThrow();
    expect(() => readHeader(new ArrayBuffer(MAX_FRAME + 1))).toThrow(/over the 34 MiB/);
    // A 5 MiB storage.set: over the default budget, refused on its size alone.
    const big = forge(3, 7, "storage.set", [0], FRAME_BUDGET.default + 1);
    expect(() => readHeader(big)).toThrow(/over its 4096 KiB budget/);
    // The header is still readable, so the refusal can be answered.
    expect(peekFrame(big)).toMatchObject({ kind: "req", id: 7, name: "storage.set" });
    // file.save may be bigger.
    expect(() => readHeader(forge(3, 7, "file.save", [0], FRAME_BUDGET.default + 1))).not.toThrow();
  });

  it("refuse a count the bytes cannot hold before allocating anything for it", () => {
    // A list claiming 2^32-1 items in a 30-byte frame.
    const list = forge(3, 1, "model.stats", [7, ...u32(0xffffffff)]);
    const t0 = performance.now();
    expect(() => decodeFrame(list)).toThrow(/a list of 4294967295 items, more than the message holds/);
    const obj = forge(3, 1, "model.stats", [8, ...u32(1_000_000)]);
    expect(() => decodeFrame(obj)).toThrow(/an object of 1000000 fields/);
    const str = forge(3, 1, "model.stats", [5, ...u32(0x7fffffff)]);
    expect(() => decodeFrame(str)).toThrow(/longer than|past the end/);
    const bytes = forge(3, 1, "model.stats", [6, ...u32(0x7fffffff)]);
    expect(() => decodeFrame(bytes)).toThrow(/more than 33554432 bytes/);
    expect(performance.now() - t0).toBeLessThan(50);
  });

  it("refuse more than a million values, nesting deeper than 64, long keys, __proto__, repeated keys, bad UTF-8 and trailing bytes", () => {
    // Two lists of 500,001 nulls: each list is fine, the message is over a million values.
    const half = WIRE_LIMITS.nodes / 2 + 1;
    const many = new Uint8Array(5 + 2 * (5 + half));
    many.set([7, ...u32(2), 7, ...u32(half)]);
    many.set([7, ...u32(half)], 10 + half);
    expect(() => decodeFrame(forge(3, 1, "model.stats", many))).toThrow(/more than 1000000 values/);
    const deep: number[] = [];
    for (let i = 0; i < 70; i++) deep.push(7, ...u32(1));
    deep.push(0);
    expect(() => decodeFrame(forge(3, 1, "model.stats", deep))).toThrow(/nested more than 64 deep/);
    const key = (k: string) => [...u32(k.length), ...[...k].map((c) => c.charCodeAt(0))];
    expect(() => decodeFrame(forge(3, 1, "model.stats", [8, ...u32(1), ...key("k".repeat(800)), 0]))).toThrow(/key that is longer than 256/);
    expect(() => decodeFrame(forge(3, 1, "model.stats", [8, ...u32(1), ...key("__proto__"), 0]))).toThrow(/__proto__/);
    expect(() => decodeFrame(forge(3, 1, "model.stats", [8, ...u32(2), ...key("a"), 0, ...key("a"), 0]))).toThrow(/twice/);
    expect(() => decodeFrame(forge(3, 1, "model.stats", [5, ...u32(2), 0xff, 0xfe]))).toThrow(/not UTF-8/);
    expect(() => decodeFrame(forge(3, 1, "model.stats", [0, 0]))).toThrow(/bytes after its value/);
    expect(() => decodeFrame(forge(3, 1, "model.stats", [3, 0, 0, 0, 0, 0, 0, 0xf0, 0x7f]))).toThrow(/not a finite number/);
  });

  it("decode byte arrays into their own buffers (nothing keeps the frame alive)", () => {
    const f = encodeHostMessage({ t: "res", id: 1, ok: true, value: { body: new Uint8Array(1000).fill(7) }, method: "net.fetch" });
    const m = decodeHostFrame(f) as unknown as { value: { body: Uint8Array } };
    expect(m.value.body.byteLength).toBe(1000);
    expect(m.value.body.buffer.byteLength).toBe(1000);
  });

  it("a plugin cannot send host kinds, nor the host plugin kinds", () => {
    expect(() => decodePluginFrame(encodeHostMessage({ t: "ping", n: 1 }))).toThrow(/cannot send a ping/);
    expect(() => decodeHostFrame(encodePluginMessage({ t: "pong", n: 1 }))).toThrow(/cannot send a pong/);
  });
});
