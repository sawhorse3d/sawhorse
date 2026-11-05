// The desktop's Save dialog hands the name back as typed on Linux (GTK's chooser adds no
// extension; Windows and macOS add the filter's). A name without the type's extension gets
// it before the file is written, so the Open dialog's filter finds it again.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { type FileType, saveFile, SHZ, withExtension } from "./files";

/** An export type, as the STL exporter names it. */
const STL: FileType = { description: "STL", mime: "model/stl", extensions: [".stl"] };

const dialog = vi.hoisted(() => ({ save: vi.fn<() => Promise<string | null>>() }));
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
    expect(r).toEqual({ name: "cabinet.shz", handle: { kind: "tauri", path: "/home/me/cabinet.shz" } });
  });

  it("a name chosen with the extension is saved as it is", async () => {
    dialog.save.mockResolvedValue("/home/me/Cabinet.SHZ");
    const r = await saveFile("Untitled.shz", bytes, SHZ);
    expect(written()).toBe("/home/me/Cabinet.SHZ");
    expect(r?.name).toBe("Cabinet.SHZ");
  });

  it("a cancelled dialog saves nothing", async () => {
    dialog.save.mockResolvedValue(null);
    expect(await saveFile("Untitled.shz", bytes, SHZ)).toBeNull();
    expect(core.invoke).not.toHaveBeenCalled();
  });

  it("saving again goes to the handle's path without a dialog", async () => {
    const r = await saveFile("cabinet.shz", bytes, SHZ, { kind: "tauri", path: "/home/me/cabinet.shz" });
    expect(dialog.save).not.toHaveBeenCalled();
    expect(written()).toBe("/home/me/cabinet.shz");
    expect(r?.name).toBe("cabinet.shz");
  });
});
