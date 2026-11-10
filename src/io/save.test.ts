// The desktop's Save dialog hands the name back as typed on Linux (GTK's chooser adds no
// extension; Windows and macOS add the filter's). A name without the type's extension gets
// it before the file is written, so the Open dialog's filter finds it again.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CSV_TYPE } from "../cutlist/csv";
import { PDF_TYPE } from "../cutlist/pdf";
import { DXF_TYPE as NEST_DXF } from "../nesting/dxf";
import { SVG_TYPE as NEST_SVG } from "../nesting/svg";
import { DXF_TYPE as SHEET_DXF, PNG_TYPE as SHEET_PNG, SVG_TYPE as SHEET_SVG } from "../sheets/runExport";
import { EXPORTS, MTL_TYPE, textureType } from "./exports";
import { type FileType, saveFile, SHZ, withExtension } from "./files";

/** An export type, as the STL exporter names it. */
const STL: FileType = { description: "STL", mime: "model/stl", extensions: [".stl"] };

const dialog = vi.hoisted(() => ({ save: vi.fn<() => Promise<string | null>>(), confirm: vi.fn<(message: string, options?: object) => Promise<boolean>>() }));
const core = vi.hoisted(() => ({ invoke: vi.fn<(cmd: string, body: unknown, opts: { headers: Record<string, string> }) => Promise<void>>() }));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
vi.mock("@tauri-apps/api/core", () => core);

describe("withExtension", () => {
  it("leaves a name that has one of the type's extensions, in any case", () => {
    expect(withExtension("/home/me/cabinet.shz", SHZ)).toBe("/home/me/cabinet.shz");
    expect(withExtension("C:\\Models\\Cabinet.SHZ", SHZ)).toBe("C:\\Models\\Cabinet.SHZ");
    expect(withExtension("/home/me/part.stl", STL)).toBe("/home/me/part.stl");
  });

  it("appends the type's first extension to a name without one, or with another", () => {
    expect(withExtension("/home/me/cabinet", SHZ)).toBe("/home/me/cabinet.shz");
    expect(withExtension("/home/me/cabinet.v2", SHZ)).toBe("/home/me/cabinet.v2.shz");
    expect(withExtension("/home/me/part", STL)).toBe("/home/me/part.stl");
  });
});

describe("saving in the desktop app", () => {
  const w = window as unknown as Record<string, unknown>;
  const bytes = new Uint8Array([80, 75, 3, 4]);
  beforeEach(() => {
    w.__TAURI_INTERNALS__ = {};
    core.invoke.mockResolvedValue(undefined);
  });
  afterEach(() => {
    delete w.__TAURI_INTERNALS__;
    dialog.save.mockReset();
    dialog.confirm.mockReset();
    core.invoke.mockReset();
  });

  /** The path the shell was asked to write, from the invoke's percent-encoded header. */
  const written = () => decodeURIComponent(core.invoke.mock.calls[0][2].headers["x-path"]);

  it("a name chosen without the extension is saved with it, and the handle keeps that name", async () => {
    dialog.save.mockResolvedValue("/home/me/cabinet");
    const r = await saveFile("Untitled.shz", bytes, SHZ);
    expect(dialog.save).toHaveBeenCalledWith({ defaultPath: "Untitled.shz", filters: [{ name: "Sawhorse document", extensions: ["shz"] }] });
    expect(core.invoke).toHaveBeenCalledOnce();
    expect(core.invoke.mock.calls[0][0]).toBe("save_file_atomic");
    expect(core.invoke.mock.calls[0][1]).toBe(bytes);
    expect(written()).toBe("/home/me/cabinet.shz");
    // The dialog's own path travels with the save, so the shell can see the file is the person's.
    expect(core.invoke.mock.calls[0][2].headers).toEqual({ "x-path": encodeURIComponent("/home/me/cabinet.shz"), "x-chosen": encodeURIComponent("/home/me/cabinet") });
    expect(r).toEqual({ name: "cabinet.shz", handle: { kind: "tauri", path: "/home/me/cabinet.shz", chosen: "/home/me/cabinet" } });
  });

  it("a name chosen with the extension is saved as it is", async () => {
    dialog.save.mockResolvedValue("/home/me/Cabinet.SHZ");
    const r = await saveFile("Untitled.shz", bytes, SHZ);
    expect(written()).toBe("/home/me/Cabinet.SHZ");
    // Nothing was added, so there is no chosen path to send: the dialog asked about replacing itself.
    expect(core.invoke.mock.calls[0][2].headers).toEqual({ "x-path": encodeURIComponent("/home/me/Cabinet.SHZ") });
    expect(r).toEqual({ name: "Cabinet.SHZ", handle: { kind: "tauri", path: "/home/me/Cabinet.SHZ" } });
  });

  it("a cancelled dialog saves nothing", async () => {
    dialog.save.mockResolvedValue(null);
    expect(await saveFile("Untitled.shz", bytes, SHZ)).toBeNull();
    expect(core.invoke).not.toHaveBeenCalled();
  });

  it("a name that already exists once the extension is added is asked about, and No saves nothing", async () => {
    dialog.save.mockResolvedValue("/home/me/cabinet");
    core.invoke.mockRejectedValueOnce("exists:cabinet.shz");
    dialog.confirm.mockResolvedValue(false);
    expect(await saveFile("Untitled.shz", bytes, SHZ)).toBeNull();
    expect(dialog.confirm).toHaveBeenCalledWith("cabinet.shz already exists. Replace it?", expect.objectContaining({ title: "Sawhorse" }));
    expect(core.invoke).toHaveBeenCalledOnce();
    expect(core.invoke.mock.calls[0][2].headers).not.toHaveProperty("x-replace");
  });

  it("Yes sends the save again with consent, and the file is replaced", async () => {
    dialog.save.mockResolvedValue("/home/me/cabinet");
    core.invoke.mockRejectedValueOnce(new Error("exists:cabinet.shz")).mockResolvedValueOnce(undefined);
    dialog.confirm.mockResolvedValue(true);
    const r = await saveFile("Untitled.shz", bytes, SHZ);
    expect(core.invoke).toHaveBeenCalledTimes(2);
    expect(core.invoke.mock.calls[1][2].headers).toEqual({ "x-path": encodeURIComponent("/home/me/cabinet.shz"), "x-chosen": encodeURIComponent("/home/me/cabinet"), "x-replace": "yes" });
    expect(r).toEqual({ name: "cabinet.shz", handle: { kind: "tauri", path: "/home/me/cabinet.shz", chosen: "/home/me/cabinet" } });
  });

  it("any other refusal from the shell is an error, not a question", async () => {
    dialog.save.mockResolvedValue("/home/me/cabinet");
    core.invoke.mockRejectedValueOnce("/home/me/cabinet.shz was not chosen in a Save dialog");
    await expect(saveFile("Untitled.shz", bytes, SHZ)).rejects.toMatch(/not chosen/);
    expect(dialog.confirm).not.toHaveBeenCalled();
  });

  it("saving again goes to the handle's path without a dialog", async () => {
    const r = await saveFile("cabinet.shz", bytes, SHZ, { kind: "tauri", path: "/home/me/cabinet.shz" });
    expect(dialog.save).not.toHaveBeenCalled();
    expect(core.invoke.mock.calls[0][2].headers).toEqual({ "x-path": encodeURIComponent("/home/me/cabinet.shz") });
    expect(r).toEqual({ name: "cabinet.shz", handle: { kind: "tauri", path: "/home/me/cabinet.shz" } });
  });

  it("saving again to a file whose extension was added here sends the chosen path and consent, and asks nothing", async () => {
    const r = await saveFile("cabinet.shz", bytes, SHZ, { kind: "tauri", path: "/home/me/cabinet.shz", chosen: "/home/me/cabinet" });
    expect(dialog.save).not.toHaveBeenCalled();
    expect(dialog.confirm).not.toHaveBeenCalled();
    expect(core.invoke.mock.calls[0][2].headers).toEqual({ "x-path": encodeURIComponent("/home/me/cabinet.shz"), "x-chosen": encodeURIComponent("/home/me/cabinet"), "x-replace": "yes" });
    expect(r).toEqual({ name: "cabinet.shz", handle: { kind: "tauri", path: "/home/me/cabinet.shz", chosen: "/home/me/cabinet" } });
  });
});

// Lane 32, the maintainer's checks: every way the page reaches save_file_atomic, and what it
// sends on each platform. Windows' and macOS's dialogs add the filter's extension themselves,
// so the page adds nothing there and the request is the one it always was.
describe("every save the page sends to the shell", () => {
  const w = window as unknown as Record<string, unknown>;
  const bytes = new Uint8Array([80, 75, 3, 4]);
  beforeEach(() => {
    w.__TAURI_INTERNALS__ = {};
    core.invoke.mockResolvedValue(undefined);
  });
  afterEach(() => {
    delete w.__TAURI_INTERNALS__;
    dialog.save.mockReset();
    dialog.confirm.mockReset();
    core.invoke.mockReset();
  });
  const headers = (call = 0) => core.invoke.mock.calls[call][2].headers;
  const only = (path: string) => ({ "x-path": encodeURIComponent(path) });

  /** Every file type the app saves with: the document, each export, the OBJ's companions, the cut list, the nesting and the sheets. */
  const TYPES: FileType[] = [SHZ, ...EXPORTS.map((c) => c.type), MTL_TYPE, textureType("image/png"), textureType("image/jpeg"), CSV_TYPE, PDF_TYPE, NEST_DXF, NEST_SVG, SHEET_DXF, SHEET_SVG, SHEET_PNG];

  it("every type's extension is one the shell takes as added (1 to 10 letters or digits)", () => {
    // Otherwise a name typed without it on Linux would be refused as not chosen.
    for (const t of TYPES) for (const e of t.extensions) expect(e, t.description).toMatch(/^\.[A-Za-z0-9]{1,10}$/);
  });

  it("a dialog that added the extension itself (Windows, macOS): the request is exactly the old one, for every type", async () => {
    for (const t of TYPES) {
      const path = `C:\\Users\\me\\Documents\\cabinet${t.extensions[0]}`;
      dialog.save.mockResolvedValueOnce(path);
      const r = await saveFile(`cabinet${t.extensions[0]}`, bytes, t);
      expect(headers(core.invoke.mock.calls.length - 1), t.description).toEqual(only(path));
      expect(r, t.description).toEqual({ name: `cabinet${t.extensions[0]}`, handle: { kind: "tauri", path } });
    }
    expect(dialog.confirm).not.toHaveBeenCalled();
  });

  it("a name typed without the extension (Linux): the type's extension is added and the dialog's path goes with it, for every type", async () => {
    for (const t of TYPES) {
      dialog.save.mockResolvedValueOnce("/home/me/cabinet");
      const r = await saveFile(`cabinet${t.extensions[0]}`, bytes, t);
      const path = `/home/me/cabinet${t.extensions[0]}`;
      expect(headers(core.invoke.mock.calls.length - 1), t.description).toEqual({ ...only(path), "x-chosen": encodeURIComponent("/home/me/cabinet") });
      expect(r?.handle, t.description).toEqual({ kind: "tauri", path, chosen: "/home/me/cabinet" });
    }
  });

  it("the extension is the name's own, never a folder's, and a typed dot stays as typed", async () => {
    dialog.save.mockResolvedValueOnce("/home/me.d/cabinet");
    await saveFile("Untitled.shz", bytes, SHZ);
    expect(headers(0)).toEqual({ ...only("/home/me.d/cabinet.shz"), "x-chosen": encodeURIComponent("/home/me.d/cabinet") });
    dialog.save.mockResolvedValueOnce("C:\\Models.v2\\cabinet");
    await saveFile("Untitled.shz", bytes, SHZ);
    expect(headers(1)).toEqual({ ...only("C:\\Models.v2\\cabinet.shz"), "x-chosen": encodeURIComponent("C:\\Models.v2\\cabinet") });
    // "cabinet." is the typed name; the shell's rule sees "cabinet." plus "shz" too.
    dialog.save.mockResolvedValueOnce("/home/me/cabinet.");
    await saveFile("Untitled.shz", bytes, SHZ);
    expect(headers(2)).toEqual({ ...only("/home/me/cabinet..shz"), "x-chosen": encodeURIComponent("/home/me/cabinet.") });
    // A hidden ".shz" already ends in the extension: written as it is, as before.
    dialog.save.mockResolvedValueOnce("/home/me/.shz");
    await saveFile("Untitled.shz", bytes, SHZ);
    expect(headers(3)).toEqual(only("/home/me/.shz"));
  });

  it("Ctrl+S on a file opened, dropped or saved as it was named writes that path exactly, whatever its name", async () => {
    // Only a name from the Save dialog ever gets an extension; a file opened with none keeps none.
    for (const path of ["/home/me/cabinet.shz", "/home/me/cabinet", "C:\\Models\\Cabinet.SHZ"]) {
      core.invoke.mockClear();
      const r = await saveFile("cabinet.shz", bytes, SHZ, { kind: "tauri", path });
      expect(headers(0), path).toEqual(only(path));
      expect(r, path).toEqual({ name: path.replace(/^.*[\\/]/, ""), handle: { kind: "tauri", path } });
    }
    expect(dialog.save).not.toHaveBeenCalled();
    expect(dialog.confirm).not.toHaveBeenCalled();
  });

  it("Ctrl+S on a handle with a chosen path that the shell still refuses is an error, never a question", async () => {
    core.invoke.mockRejectedValueOnce("/home/me/cabinet.shz was not chosen in a Save dialog");
    await expect(saveFile("cabinet.shz", bytes, SHZ, { kind: "tauri", path: "/home/me/cabinet.shz", chosen: "/home/me/cabinet" })).rejects.toMatch(/not chosen/);
    expect(dialog.confirm).not.toHaveBeenCalled();
    expect(core.invoke).toHaveBeenCalledOnce();
  });

  it("Save As after such a save starts again from the dialog: a name typed with the extension has no chosen path", async () => {
    dialog.save.mockResolvedValueOnce("/home/me/bench.shz");
    const r = await saveFile("cabinet.shz", bytes, SHZ);
    expect(headers(0)).toEqual(only("/home/me/bench.shz"));
    expect(r).toEqual({ name: "bench.shz", handle: { kind: "tauri", path: "/home/me/bench.shz" } });
  });

  it("Yes, then another refusal on the resend, is an error and asks only once", async () => {
    dialog.save.mockResolvedValue("/home/me/cabinet");
    core.invoke.mockRejectedValueOnce("exists:cabinet.shz").mockRejectedValueOnce("could not write the file: disk full");
    dialog.confirm.mockResolvedValue(true);
    await expect(saveFile("Untitled.shz", bytes, SHZ)).rejects.toMatch(/disk full/);
    expect(dialog.confirm).toHaveBeenCalledOnce();
    expect(core.invoke).toHaveBeenCalledTimes(2);
  });

  it("a save given up while the question is open writes nothing", async () => {
    dialog.save.mockResolvedValue("/home/me/cabinet");
    core.invoke.mockRejectedValueOnce("exists:cabinet.shz");
    const ctl = new AbortController();
    dialog.confirm.mockImplementation(async () => (ctl.abort(), true));
    expect(await saveFile("Untitled.shz", bytes, SHZ, undefined, { signal: ctl.signal })).toBeNull();
    expect(core.invoke).toHaveBeenCalledOnce();
  });

  it("a plugin's file.save (no handle, only the dialog's answer) is asked about like any Save As, and never sends consent by itself", async () => {
    // The plugin bridge calls saveFile(name, bytes, type, undefined, { confirmDownload, signal }):
    // the headers come from the dialog's path alone.
    const md: FileType = { description: "MD file", mime: "text/markdown", extensions: [".md"] };
    dialog.save.mockResolvedValue("/home/me/report");
    core.invoke.mockRejectedValueOnce("exists:report.md");
    dialog.confirm.mockResolvedValue(false);
    expect(await saveFile("report.md", bytes, md, undefined, { confirmDownload: async () => true })).toBeNull();
    expect(headers(0)).toEqual({ ...only("/home/me/report.md"), "x-chosen": encodeURIComponent("/home/me/report") });
    expect(dialog.confirm).toHaveBeenCalledWith("report.md already exists. Replace it?", expect.anything());
    expect(core.invoke).toHaveBeenCalledOnce();
  });
});
