// #71, #80: the desktop's Open and Save dialogs are given a folder to start in. Which of
// the folders named is still there is the shell's to say (src-tauri/dialog-folder, cargo
// test -p sawhorse-dialog-folder); this is the page's side: what it keeps, what it names,
// and what each dialog is then given.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { pickFile, saveFile, SHZ, takeTauriPaths, type FileType } from "./files";
import { ASK_MS, FOLDER_KEY, folderOf, inFolder, keptFolder, MAX_FOLDER, rememberFolder, startFolder } from "./folder";
import { pluginPlatform } from "../plugins/platform";

const dialog = vi.hoisted(() => ({ open: vi.fn<(o: Record<string, unknown>) => Promise<string | null>>(), save: vi.fn<(o: Record<string, unknown>) => Promise<string | null>>() }));
const core = vi.hoisted(() => ({ invoke: vi.fn<(cmd: string, args?: unknown, opts?: unknown) => Promise<unknown>>() }));
const fs = vi.hoisted(() => ({ stat: vi.fn<(path: string) => Promise<{ size: number }>>(), readFile: vi.fn<(path: string) => Promise<Uint8Array>>(), readDir: vi.fn<(path: string) => Promise<unknown[]>>() }));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
vi.mock("@tauri-apps/api/core", () => core);
vi.mock("@tauri-apps/plugin-fs", () => fs);

const w = window as unknown as Record<string, unknown>;
const bytes = new Uint8Array([80, 75, 3, 4]);
const STL: FileType = { description: "STL", mime: "model/stl", extensions: [".stl"] };

/** A shell that answers dialog_folder with `folder` and takes every save. */
function shell(folder: string | null) {
  core.invoke.mockImplementation(async (cmd) => (cmd === "dialog_folder" ? folder : undefined));
}
/** What the shell was asked: the folders named, one list per dialog. */
const asked = () => core.invoke.mock.calls.filter((c) => c[0] === "dialog_folder").map((c) => (c[1] as { folders: string[] }).folders);

beforeEach(() => {
  w.__TAURI_INTERNALS__ = {};
  fs.stat.mockResolvedValue({ size: 4 });
  fs.readFile.mockResolvedValue(bytes);
  fs.readDir.mockResolvedValue([]);
});
afterEach(() => {
  delete w.__TAURI_INTERNALS__;
  localStorage.removeItem(FOLDER_KEY);
  dialog.open.mockReset();
  dialog.save.mockReset();
  core.invoke.mockReset();
  fs.stat.mockReset();
  fs.readFile.mockReset();
  fs.readDir.mockReset();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("a file's folder", () => {
  it("is the path without its last name", () => {
    expect(folderOf("/home/maker/models/shelf.shz")).toBe("/home/maker/models");
    expect(folderOf("/home/maker/two words/shelf v2.shz")).toBe("/home/maker/two words");
    expect(folderOf("C:\\Users\\maker\\Documents\\shelf.shz")).toBe("C:\\Users\\maker\\Documents");
  });

  it("is the root for a file at the root, with its separator", () => {
    expect(folderOf("/shelf.shz")).toBe("/");
    expect(folderOf("C:\\shelf.shz")).toBe("C:\\");
    expect(folderOf("C:/shelf.shz")).toBe("C:/");
  });

  it("is nothing for a bare name", () => {
    expect(folderOf("shelf.shz")).toBeNull();
    expect(folderOf("")).toBeNull();
  });

  it("takes a name back with the separator it is written with", () => {
    expect(inFolder("/home/maker/models", "Untitled.shz")).toBe("/home/maker/models/Untitled.shz");
    expect(inFolder("/", "Untitled.shz")).toBe("/Untitled.shz");
    expect(inFolder("C:\\Users\\maker", "Untitled.shz")).toBe("C:\\Users\\maker\\Untitled.shz");
    expect(inFolder("C:\\", "Untitled.shz")).toBe("C:\\Untitled.shz");
  });
});

describe("the folder kept between dialogs", () => {
  it("is the folder of the last file, under the app's own key", () => {
    expect(keptFolder()).toBeNull();
    rememberFolder("/home/maker/models/shelf.shz");
    expect(localStorage.getItem(FOLDER_KEY)).toBe("/home/maker/models");
    expect(FOLDER_KEY).toBe("sawhorse.lastFolder");
    rememberFolder("/home/maker/Downloads/part.stl");
    expect(keptFolder()).toBe("/home/maker/Downloads");
  });

  it("a bare name or a path too long to be one keeps nothing, and what was kept stays", () => {
    rememberFolder("/home/maker/models/shelf.shz");
    rememberFolder("shelf.shz");
    rememberFolder(`/${"a".repeat(MAX_FOLDER)}/shelf.shz`);
    expect(keptFolder()).toBe("/home/maker/models");
  });

  it("a value too long in storage is not a folder", () => {
    localStorage.setItem(FOLDER_KEY, `/${"a".repeat(MAX_FOLDER)}`);
    expect(keptFolder()).toBeNull();
  });

  it("storage that cannot be read or written stops nothing", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("storage is off");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("storage is full");
    });
    expect(keptFolder()).toBeNull();
    expect(() => rememberFolder("/home/maker/models/shelf.shz")).not.toThrow();
  });
});

describe("asking the shell where a dialog starts", () => {
  it("names the folder kept, and gives the shell's answer", async () => {
    rememberFolder("/home/maker/models/shelf.shz");
    shell("/home/maker/models");
    expect(await startFolder()).toBe("/home/maker/models");
    expect(core.invoke).toHaveBeenCalledWith("dialog_folder", { folders: ["/home/maker/models"] });
  });

  it("names the folder of the model's own file first", async () => {
    rememberFolder("/home/maker/Downloads/part.stl");
    shell("/home/maker/models");
    await startFolder("/home/maker/models/shelf.shz");
    expect(asked()).toEqual([["/home/maker/models", "/home/maker/Downloads"]]);
  });

  it("with nothing kept it still asks: the shell has the Documents and home folders", async () => {
    shell("/home/maker/Documents");
    expect(await startFolder()).toBe("/home/maker/Documents");
    expect(asked()).toEqual([[]]);
  });

  it("no folder from the shell (Windows, macOS, or none there) is none", async () => {
    for (const answer of [null, undefined, "", 7, {}]) {
      core.invoke.mockResolvedValueOnce(answer);
      expect(await startFolder(), String(answer)).toBeNull();
    }
  });

  it("a shell without the command is no folder, not an error", async () => {
    core.invoke.mockRejectedValue(new Error("dialog_folder is not in this shell"));
    expect(await startFolder()).toBeNull();
    core.invoke.mockImplementation(() => {
      throw new TypeError("no bridge");
    });
    expect(await startFolder()).toBeNull();
  });

  it("a shell that does not answer is given up on, and the dialog can open", async () => {
    vi.useFakeTimers();
    core.invoke.mockReturnValue(new Promise(() => {}));
    let answer: string | null | undefined;
    void startFolder().then((a) => (answer = a));
    await vi.advanceTimersByTimeAsync(ASK_MS - 1);
    expect(answer).toBeUndefined();
    await vi.advanceTimersByTimeAsync(1);
    expect(answer).toBeNull();
  });
});

describe("File > Open in the desktop app", () => {
  it("starts in the folder the shell gives, and keeps the folder of the file picked", async () => {
    rememberFolder("/home/maker/models/shelf.shz");
    shell("/home/maker/models");
    dialog.open.mockResolvedValue("/home/maker/other/bench.shz");
    const f = await pickFile([SHZ]);
    expect(dialog.open).toHaveBeenCalledWith({ multiple: false, directory: false, filters: [{ name: "Supported files", extensions: ["shz"] }], defaultPath: "/home/maker/models" });
    expect(f?.name).toBe("bench.shz");
    expect(keptFolder()).toBe("/home/maker/other");
  });

  it("with no folder from the shell the dialog is given none, as before", async () => {
    shell(null);
    dialog.open.mockResolvedValue("/home/maker/models/shelf.shz");
    await pickFile([SHZ, STL]);
    expect(dialog.open).toHaveBeenCalledWith({ multiple: false, directory: false, filters: [{ name: "Supported files", extensions: ["shz", "stl"] }] });
    expect(dialog.open.mock.calls[0][0]).not.toHaveProperty("defaultPath");
  });

  it("a cancelled dialog keeps no folder", async () => {
    rememberFolder("/home/maker/models/shelf.shz");
    shell("/home/maker/models");
    dialog.open.mockResolvedValue(null);
    expect(await pickFile([SHZ])).toBeNull();
    expect(keptFolder()).toBe("/home/maker/models");
  });

  it("a pick given up before the dialog opens opens none", async () => {
    const ctl = new AbortController();
    core.invoke.mockImplementation(async () => (ctl.abort(), "/home/maker"));
    expect(await pickFile([SHZ], { signal: ctl.signal })).toBeNull();
    expect(dialog.open).not.toHaveBeenCalled();
  });
});

describe("Save in the desktop app", () => {
  it("the name goes to the dialog in the folder the shell gives, and the saved file's folder is kept", async () => {
    shell("/home/maker/Documents");
    dialog.save.mockResolvedValue("/home/maker/models/shelf.shz");
    await saveFile("Untitled.shz", bytes, SHZ);
    expect(dialog.save).toHaveBeenCalledWith({ defaultPath: "/home/maker/Documents/Untitled.shz", filters: [{ name: "Sawhorse document", extensions: ["shz"] }] });
    expect(keptFolder()).toBe("/home/maker/models");
    // The save itself is the one it was.
    const saves = core.invoke.mock.calls.filter((c) => c[0] === "save_file_atomic");
    expect(saves).toHaveLength(1);
    expect((saves[0][2] as { headers: Record<string, string> }).headers).toEqual({ "x-path": encodeURIComponent("/home/maker/models/shelf.shz") });
  });

  it("Save As names the folder of the file the model has before the one kept", async () => {
    rememberFolder("/home/maker/Downloads/part.stl");
    shell("/home/maker/models");
    dialog.save.mockResolvedValue("/home/maker/models/shelf v2.shz");
    await saveFile("shelf.shz", bytes, SHZ, undefined, { beside: { kind: "tauri", path: "/home/maker/models/shelf.shz" } });
    expect(asked()).toEqual([["/home/maker/models", "/home/maker/Downloads"]]);
    expect(dialog.save.mock.calls[0][0].defaultPath).toBe("/home/maker/models/shelf.shz");
  });

  it("an export after it starts in the folder kept", async () => {
    rememberFolder("/home/maker/models/shelf.shz");
    shell("/home/maker/models");
    dialog.save.mockResolvedValue("/home/maker/models/shelf.stl");
    await saveFile("shelf.stl", bytes, STL);
    expect(asked()).toEqual([["/home/maker/models"]]);
    expect(dialog.save.mock.calls[0][0].defaultPath).toBe("/home/maker/models/shelf.stl");
  });

  it("with no folder from the shell the dialog is given the bare name, as before", async () => {
    shell(null);
    dialog.save.mockResolvedValue("C:\\Users\\maker\\Documents\\shelf.shz");
    await saveFile("Untitled.shz", bytes, SHZ);
    expect(dialog.save.mock.calls[0][0].defaultPath).toBe("Untitled.shz");
    expect(keptFolder()).toBe("C:\\Users\\maker\\Documents");
  });

  it("a name that is itself a path goes to the dialog as it is, and the shell is not asked", async () => {
    shell("/home/maker/Documents");
    dialog.save.mockResolvedValue("/home/maker/report.md");
    const md: FileType = { description: "MD file", mime: "text/markdown", extensions: [".md"] };
    await saveFile("out/report.md", bytes, md);
    expect(asked()).toEqual([]);
    expect(dialog.save.mock.calls[0][0].defaultPath).toBe("out/report.md");
  });

  it("a cancelled dialog keeps no folder and saves nothing", async () => {
    shell("/home/maker/Documents");
    dialog.save.mockResolvedValue(null);
    expect(await saveFile("Untitled.shz", bytes, SHZ)).toBeNull();
    expect(keptFolder()).toBeNull();
    expect(core.invoke.mock.calls.filter((c) => c[0] === "save_file_atomic")).toHaveLength(0);
  });

  it("saving again to the model's file opens no dialog and asks the shell for no folder", async () => {
    shell("/home/maker/Documents");
    await saveFile("shelf.shz", bytes, SHZ, { kind: "tauri", path: "/home/maker/models/shelf.shz" });
    expect(dialog.save).not.toHaveBeenCalled();
    expect(asked()).toEqual([]);
  });
});

describe("a file dropped on the window, or the one the app was started with", () => {
  it("keeps its folder once it is read", async () => {
    await takeTauriPaths(["/home/maker/models/shelf.shz"], () => {});
    expect(keptFolder()).toBe("/home/maker/models");
  });

  it("a file that could not be read, or is not one Sawhorse opens, keeps none", async () => {
    fs.stat.mockRejectedValue(new Error("no such file"));
    await takeTauriPaths(["/home/maker/gone/shelf.shz", "/home/maker/notes/readme.txt"], () => {}, () => {});
    expect(keptFolder()).toBeNull();
  });
});

describe("Load an unpacked plugin folder, in the desktop app", () => {
  const options = { directory: true, multiple: false, recursive: true, title: "Load an unpacked plugin folder" };

  it("starts where Open starts", async () => {
    rememberFolder("/home/maker/models/shelf.shz");
    shell("/home/maker/models");
    dialog.open.mockResolvedValue(null);
    expect(await pluginPlatform().pickFolder?.()).toBeNull();
    expect(asked()).toEqual([["/home/maker/models"]]);
    expect(dialog.open).toHaveBeenCalledWith({ ...options, defaultPath: "/home/maker/models" });
  });

  it("with no folder from the shell the dialog is given none, as before", async () => {
    shell(null);
    dialog.open.mockResolvedValue(null);
    await pluginPlatform().pickFolder?.();
    expect(dialog.open).toHaveBeenCalledWith(options);
  });

  it("the folder picked is not kept: a plugin's folder is not where the models are", async () => {
    rememberFolder("/home/maker/models/shelf.shz");
    shell("/home/maker/models");
    dialog.open.mockResolvedValue("/home/maker/code/my-plugin");
    // An empty folder is no plugin; what matters here is what was kept.
    await pluginPlatform().pickFolder?.().catch(() => null);
    expect(fs.readDir).toHaveBeenCalledWith("/home/maker/code/my-plugin");
    expect(keptFolder()).toBe("/home/maker/models");
  });
});

describe("where the folder is written", () => {
  it("is one key in this browser's storage, and nothing is logged", async () => {
    const logs = (["log", "info", "warn", "error", "debug"] as const).map((m) => vi.spyOn(console, m).mockImplementation(() => {}));
    const before = Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i));
    shell("/home/maker/Documents");
    dialog.open.mockResolvedValue("/home/maker/models/shelf.shz");
    await pickFile([SHZ]);
    dialog.save.mockResolvedValue("/home/maker/other/bench.shz");
    await saveFile("Untitled.shz", bytes, SHZ);
    await takeTauriPaths(["/home/maker/third/stool.shz"], () => {});
    expect(keptFolder()).toBe("/home/maker/third");
    const after = Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i));
    expect(after.filter((k) => !before.includes(k))).toEqual([FOLDER_KEY]);
    expect(sessionStorage.length).toBe(0);
    for (const l of logs) expect(l).not.toHaveBeenCalled();
  });

  it("the bytes saved are the bytes given: the folder goes to the dialog, not into the file", async () => {
    shell("/home/maker/Documents");
    dialog.save.mockResolvedValue("/home/maker/models/shelf.shz");
    await saveFile("Untitled.shz", bytes, SHZ);
    const saves = core.invoke.mock.calls.filter((c) => c[0] === "save_file_atomic");
    expect(saves).toHaveLength(1);
    expect(saves[0][1]).toBe(bytes);
  });
});

describe("the browser build", () => {
  it("keeps no folder and asks no shell: a save with no desktop bridge goes to the browser", async () => {
    delete w.__TAURI_INTERNALS__;
    const clicked = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    (URL as unknown as { createObjectURL: unknown }).createObjectURL ??= () => "blob:x";
    (URL as unknown as { revokeObjectURL: unknown }).revokeObjectURL ??= () => {};
    await saveFile("Untitled.shz", bytes, SHZ);
    expect(clicked).toHaveBeenCalledOnce();
    expect(core.invoke).not.toHaveBeenCalled();
    expect(keptFolder()).toBeNull();
  });
});
