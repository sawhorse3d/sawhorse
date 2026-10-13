// Lane 16: plugin packages and SketchUp files are zips the person picked; what they claim
// is checked before anything is inflated, and what they hold is checked against what they
// claim.
import { strToU8, zipSync } from "fflate";
import { describe, expect, it } from "vitest";
import { checkSkpArchive, SkpTooLarge } from "../io/skp";
import { crc32, zipDirectory } from "../lib/zipdir";
import { deflateZeros, rawZip } from "../test/zips";
import { filesFromEntries, PackageError, readPluginZip, writePluginZip } from "./zip";

const GB2 = 2 * 1024 * 1024 * 1024;

describe("zip bombs", () => {
  const bomb = deflateZeros(GB2);

  it("are about 2 MB that inflate to 2 GB", () => {
    expect(bomb.length).toBeLessThan(4 * 1024 * 1024);
  });

  it("a plugin package that declares 2 GB is refused in under 100 ms, nothing inflated", () => {
    const zip = rawZip([
      { name: "plugin.json", method: 0, data: strToU8("{}"), size: 2, crc: crc32(strToU8("{}")) },
      { name: "main.js", method: 8, data: bomb, size: GB2 - 1 },
    ]);
    const heap = process.memoryUsage().arrayBuffers;
    const t0 = performance.now();
    expect(() => readPluginZip(zip)).toThrow(/unpacks to more than 50 MB/);
    expect(performance.now() - t0).toBeLessThan(100);
    // Nothing near its claim was allocated.
    expect(process.memoryUsage().arrayBuffers - heap).toBeLessThan(64 * 1024 * 1024);
  });

  it("a SketchUp file that declares 2 GB is refused in under 100 ms, nothing inflated", () => {
    const zip = rawZip([{ name: "model.dat", method: 8, data: bomb, size: GB2 - 1 }]);
    const heap = process.memoryUsage().arrayBuffers;
    const t0 = performance.now();
    expect(() => checkSkpArchive(zip)).toThrow(SkpTooLarge);
    expect(performance.now() - t0).toBeLessThan(100);
    expect(process.memoryUsage().arrayBuffers - heap).toBeLessThan(64 * 1024 * 1024);
    const many = rawZip(Array.from({ length: 5001 }, (_, i) => ({ name: `x${i}`, method: 0, data: new Uint8Array(), size: 0 })));
    expect(() => checkSkpArchive(many)).toThrow(/more than 5000 entries/);
    // Not a zip (an older .skp): left to openskp.
    expect(() => checkSkpArchive(new Uint8Array([0xff, 0xfe, 0xff, 0x0e]))).not.toThrow();
  });

  it("an entry longer than it declares is refused by its size and checksum (fflate would cut it short)", () => {
    const real = strToU8("export const x = 1;\n".repeat(100));
    const lie = rawZip([
      { name: "plugin.json", method: 0, data: strToU8("{}"), size: 2, crc: crc32(strToU8("{}")) },
      { name: "main.js", method: 0, data: real, size: 40, crc: crc32(real.subarray(0, 40)) },
    ]);
    expect(() => readPluginZip(lie)).toThrow(/not what the zip says it is/);
  });
});

describe("plugin package names", () => {
  const text = (s: string) => strToU8(s);
  it.each([
    ["C:/evil.js", "a drive"],
    ["lib/con.js", "a device"],
    ["plugin.json:stream", "an alternate stream"],
    ["lib/trailing. ", "a trailing dot or space"],
    ["lib/aux", "a device without an extension"],
  ])("refuses %s (%s)", (name) => {
    expect(() => filesFromEntries({ "plugin.json": text("{}"), [name]: text("x") })).toThrow(PackageError);
  });

  it("refuses two names that differ only in case", () => {
    expect(() => filesFromEntries({ "plugin.json": text("{}"), "Main.js": text("a"), "main.js": text("b") })).toThrow(/differ only in case/);
  });

  it("reads an honest package as before", () => {
    const zip = writePluginZip({ "plugin.json": "{}", "main.js": "export {};", "lib/util.js": "export const a = 1;" });
    expect(Object.keys(readPluginZip(zip).files).sort()).toEqual(["lib/util.js", "main.js", "plugin.json"]);
  });
});

describe("the zip directory reader", () => {
  it("lists names, sizes and CRCs without inflating, and refuses too many entries first", () => {
    const zip = zipSync({ "a.txt": strToU8("hello"), "b/c.txt": strToU8("") });
    const d = zipDirectory(zip, 10);
    expect(d.map((e) => [e.name, e.size, e.crc])).toEqual([
      ["a.txt", 5, crc32(strToU8("hello"))],
      ["b/c.txt", 0, 0],
    ]);
    expect(() => zipDirectory(zip, 1)).toThrow(/2 entries \(at most 1\)/);
    expect(() => zipDirectory(new Uint8Array(100), 10)).toThrow(/not a zip/);
    expect(crc32(strToU8("123456789"))).toBe(0xcbf43926);
  });
});
