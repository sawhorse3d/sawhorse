// The desktop's Save dialog hands the name back as typed on Linux (GTK's chooser adds no
// extension; Windows and macOS add the filter's). A name without the type's extension gets
// it before the file is written, so the Open dialog's filter finds it again.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
