import { describe, expect, it } from "vitest";
import { checkData, decodeHostMessage, decodePluginMessage, logText, PROTOCOL, ProtocolError, transferables } from "../src/rpc.ts";

describe("checkData", () => {
  it("accepts plain data and byte arrays", () => {
    const v = { a: [1, 2.5, -3], b: "x", c: null, d: true, e: new Uint8Array([1, 2]), f: Object.create(null) };
    expect(checkData(v)).toBe(v);
  });

  it.each([
    [{ f: () => 1 }, "value.f is a function"],
    [{ n: NaN }, "value.n is not a finite number"],
    [{ n: Infinity }, "not a finite number"],
    [{ d: new Date() }, "value.d is not a plain object (Date)"],
    [{ m: new Map() }, "not a plain object (Map)"],
    [{ u: undefined }, "value.u is a undefined"],
    [{ s: Symbol("x") }, "value.s is a symbol"],
    [[1, [2, { big: 10n }]], "value[1][1].big is a bigint"],
  ])("refuses %o", (v, msg) => {
    expect(() => checkData(v)).toThrow(ProtocolError);
    expect(() => checkData(v)).toThrow(msg);
  });

  it("refuses cycles and over-deep nesting", () => {
    const a: Record<string, unknown> = {};
    a.self = a;
    expect(() => checkData(a)).toThrow("refers back to itself");
    let deep: unknown = 1;
    for (let i = 0; i < 70; i++) deep = [deep];
    expect(() => checkData(deep)).toThrow("nested more than 64 deep");
  });

  it("lane 16: measures byte arrays by their whole buffer, refuses holes, long keys and too much in all", () => {
    const view = new Uint8Array(new ArrayBuffer(40 * 1024 * 1024), 0, 1);
    expect(() => checkData({ view })).toThrow("counting its whole buffer");
    const sparse: unknown[] = [];
    sparse.length = 2 ** 32 - 1;
    const t0 = performance.now();
    expect(() => checkData(sparse)).toThrow("a list of 4294967295 items");
    expect(performance.now() - t0).toBeLessThan(50);
    expect(() => checkData([1, , 2])).toThrow("holes or extra properties");
    expect(() => checkData({ ["k".repeat(300)]: 1 })).toThrow("key longer than 256");
    expect(() => checkData(Array.from({ length: 3 }, () => "x".repeat(2_000_000)))).toThrow("too large (more than 4096 KiB in all)");
  });

  it("allows the same object twice when it is not a cycle", () => {
    const shared = { x: 1 };
    expect(() => checkData({ a: shared, b: shared })).not.toThrow();
  });
});

describe("plugin -> host messages", () => {
  it("round-trips every kind", () => {
    const msgs = [
      { t: "ready" },
      { t: "fail", error: "boom" },
      { t: "req", id: 4, method: "model.batch", params: { name: "x", ops: [{ op: "rename", item: { object: 1 }, name: "A" }] } },
      { t: "done", id: 9, ok: true, value: [1, 2] },
      { t: "done", id: 9, ok: false, error: "nope" },
      { t: "pong", n: 3 },
      { t: "log", level: "warn", text: "careful" },
    ];
    for (const m of msgs) expect(decodePluginMessage(structuredClone(m))).toEqual(m);
  });

  it("refuses malformed messages with a reason", () => {
    expect(() => decodePluginMessage(null)).toThrow("must be an object");
    expect(() => decodePluginMessage({ t: "zap" })).toThrow('unknown message type "zap"');
    expect(() => decodePluginMessage({ t: "req", id: -1, method: "x" })).toThrow("req.id");
    expect(() => decodePluginMessage({ t: "req", id: 1.5, method: "x" })).toThrow("req.id");
    expect(() => decodePluginMessage({ t: "req", id: 1, method: "__proto__" })).toThrow("not a method name");
    expect(() => decodePluginMessage({ t: "req", id: 1, method: "model.batch", params: { f: NaN } })).toThrow("model.batch params.f is not a finite number");
    expect(() => decodePluginMessage({ t: "done", id: 1, ok: "yes" })).toThrow("done.ok");
    expect(() => decodePluginMessage({ t: "log", level: "shout", text: "x" })).toThrow("log.level");
  });

  it("caps a log line at 4,000 characters", () => {
    const m = decodePluginMessage({ t: "log", level: "log", text: "x".repeat(9000) });
    expect(m.t === "log" && m.text.length).toBe(4000);
  });
});

describe("host -> plugin messages", () => {
  it("round-trips every kind", () => {
    const msgs = [
      { t: "init", protocol: PROTOCOL, plugin: { id: "org.example.a", name: "A", version: "1.0.0", permissions: ["ui"] }, files: { "main.js": "export {}" }, entry: "main.js", units: "mm" },
      { t: "res", id: 1, ok: true, value: { solids: 3 } },
      { t: "res", id: 1, ok: false, error: "not allowed", code: "permission" },
      { t: "call", id: 2, fn: "command", args: ["generate"] },
      { t: "evt", name: "documentChanged", payload: { hash: "abc" } },
      { t: "ping", n: 7 },
    ];
    for (const m of msgs) expect(decodeHostMessage(structuredClone(m))).toEqual(m);
  });

  it("refuses an init for another protocol", () => {
    expect(() => decodeHostMessage({ t: "init", protocol: 99, plugin: {}, files: {} })).toThrow("protocol 99");
  });
});

describe("helpers", () => {
  it("lists whole byte buffers to transfer, once each", () => {
    const a = new Uint8Array(4);
    const view = new Uint8Array(new ArrayBuffer(8), 2, 2);
    expect(transferables({ a, b: [a], c: view })).toEqual([a.buffer]);
  });

  it("writes console arguments as one line", () => {
    expect(logText(["n =", 3, { a: [1] }, new Error("bad"), new Uint8Array(5)])).toBe('n = 3 {"a":[1]} Error: bad "<5 bytes>"');
  });
});
