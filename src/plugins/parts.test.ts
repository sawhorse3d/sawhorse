import { strToU8, zipSync } from "fflate";
import { describe, expect, it } from "vitest";
import { checkToolView } from "../tools/plugin";
import { loadPackage } from "./loader";
import { removeEverywhere } from "./lockdown";
import { checkBody, patchFields, valuesOf } from "./panel";
import { readPluginZip, writePluginZip } from "./zip";

const manifest = JSON.stringify({ id: "org.example.zip", name: "Zip", version: "1.0.0", api: 1, description: "", author: "Sawhorse", license: "MIT", permissions: [] });

describe("plugin zips", () => {
  it("reads a zip of the folder's contents or of the folder itself", () => {
    const flat = zipSync({ "plugin.json": strToU8(manifest), "main.js": strToU8("export {}"), "icon.png": new Uint8Array([1, 2, 3]) });
    const wrapped = zipSync({ "zip-plugin/plugin.json": strToU8(manifest), "zip-plugin/lib/a.js": strToU8("export {}"), "__MACOSX/x": strToU8("junk"), "zip-plugin/.DS_Store": strToU8("junk") });
    expect(Object.keys(readPluginZip(flat).files).sort()).toEqual(["main.js", "plugin.json"]);
    expect(readPluginZip(flat).other).toEqual({ "icon.png": 3 });
    expect(Object.keys(readPluginZip(wrapped).files).sort()).toEqual(["lib/a.js", "plugin.json"]);
    const round = readPluginZip(writePluginZip({ "plugin.json": manifest, "main.js": "export const a = 1;" }));
    expect(round.files["main.js"]).toBe("export const a = 1;");
  });

  it("refuses what is not a zip and paths that leave the folder", () => {
    expect(() => readPluginZip(new Uint8Array([1, 2, 3, 4]))).toThrow("not a readable .zip");
    expect(() => readPluginZip(zipSync({ "../evil.js": strToU8("x") }))).toThrow("outside its folder");
  });
});

describe("loadPackage", () => {
  it("keeps only the modules reachable from the entry, in import order", () => {
    const r = loadPackage({ "plugin.json": manifest, "main.js": 'import "./a.js";', "a.js": "export {}", "unused.js": "import('x')" }, "session");
    expect(r.ok && r.pkg.modules).toEqual(["a.js", "main.js"]);
  });
  it("reports a missing or broken plugin.json with the name it could find", () => {
    expect(loadPackage({ "main.js": "" }, "session")).toMatchObject({ ok: false, errors: [expect.stringContaining("no plugin.json")] });
    expect(loadPackage({ "plugin.json": '{"id":"org.x.y","name":"Half"}', "main.js": "" }, "session")).toMatchObject({ ok: false, id: "org.x.y", name: "Half" });
  });
});

describe("panel fields", () => {
  it("keeps known fields and properties only, and reports values by id", () => {
    const body = checkBody([
      { type: "number", id: "w", label: "Width", value: 0.762, unit: "length", onclick: "alert(1)" },
      { type: "checkbox", id: "back", label: "Back panel", value: 1 },
      { type: "select", id: "u", label: "Units", options: [{ value: "in", label: "Inches" }] },
      { type: "table", columns: ["Part", "Qty"], rows: [["Side", 2, "extra"]], align: ["left", "right"] },
      { type: "note", text: "hi" },
      { type: "button", id: "go", label: "Go", primary: true },
    ]);
    expect(body[0]).toEqual({ type: "number", id: "w", label: "Width", value: 0.762, unit: "length" });
    expect(body[3]).toEqual({ type: "table", columns: ["Part", "Qty"], rows: [["Side", "2"]], align: ["left", "right"] });
    expect(valuesOf(body)).toEqual({ w: 0.762, back: true, u: "in" });
    const next = patchFields(body, { fields: { go: { disabled: true } }, values: { w: 0.5 } });
    expect(next[5]).toMatchObject({ disabled: true });
    expect(valuesOf(next).w).toBe(0.5);
  });

  it.each([
    [[{ type: "script", src: "x" }], "type must be one of"],
    [[{ type: "image", src: "https://tracker.example.com/pixel.png", alt: "" }], "data: URL"],
    [[{ type: "text", id: "a", label: "A" }, { type: "text", id: "a", label: "B" }], 'two fields have the id "a"'],
    [[{ type: "number", id: "x", label: "X", value: NaN }], "finite number"],
    [[{ type: "button", id: "1go", label: "Go" }], "starting with a letter"],
  ])("refuses %j", (body, msg) => {
    expect(() => checkBody(body)).toThrow(msg);
  });
});

describe("tool views", () => {
  it("draw with the app's colours and finite points only", () => {
    const v = checkToolView({
      overlay: [
        { kind: "segments", points: [[0, 0, 0], [1, 0, 0]], color: "red" },
        { kind: "loop", points: [[0, 0, 0], [1, 1, NaN]], color: "accent" },
        { kind: "strip", points: [[0, 0, 0], [0, 1, 0]], color: "url(javascript:x)", width: 99 },
      ],
      labels: [{ at: [0, 0, 0], text: "8 holes" }],
      status: "Click to drill",
      busy: 1,
      inference: { plane: { origin: [0, 0, 0], normal: [0, 0, 1] }, lock: { dir: [1, 0, 0], kind: "nope" } },
    });
    expect(v.overlay.map((o) => o.color)).toEqual(["--axis-x", "--accent"]);
    expect(v.overlay[1].width).toBe(6);
    expect(v.labels).toHaveLength(1);
    expect(v.busy).toBe(true);
    expect(v.inference).toEqual({ plane: { origin: [0, 0, 0], normal: [0, 0, 1] } });
  });
});

describe("lockdown", () => {
  it("removes a name from an object and every prototype it inherits through", () => {
    const proto = { fetch: () => 1 };
    const mid = Object.create(proto, { XMLHttpRequest: { value: 1, configurable: true } });
    const scope = Object.create(mid);
    scope.fetch = () => 2;
    expect(removeEverywhere(scope, "fetch")).toBe(false);
    expect(removeEverywhere(scope, "XMLHttpRequest")).toBe(false);
    expect("fetch" in scope).toBe(false);
    expect(scope.XMLHttpRequest).toBeUndefined();
  });
  it("shadows what cannot be deleted", () => {
    const scope = Object.create(null);
    Object.defineProperty(scope, "fetch", { value: () => 1, configurable: false, writable: false });
    expect(removeEverywhere(scope, "fetch")).toBe(true);
  });
});
