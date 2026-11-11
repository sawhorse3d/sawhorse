// Lane 37 (#45): the file the desktop app was started with (a double-click on a .shz, or a
// path after the program's name). The shell's side, which argument is the file, is tested in
// src-tauri/start-file (cargo test -p sawhorse-start-file); this is the page's.
import { readFileSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ALL_EXTENSIONS, MAX_READ_BYTES, onTauriDrop, takeTauriPaths, type PickedFile } from "./files";
import { notOpenedNote, openLabel, START_FILE, startupOrder, takeStartupFile, waitingNote } from "./startup";

const core = vi.hoisted(() => ({ invoke: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>() }));
const fs = vi.hoisted(() => ({
  stat: vi.fn<(path: string) => Promise<{ size: number }>>(),
  readFile: vi.fn<(path: string) => Promise<Uint8Array>>(),
}));
const event = vi.hoisted(() => ({ listen: vi.fn<(name: string, cb: (e: { payload: string[] }) => unknown) => Promise<() => void>>() }));
vi.mock("@tauri-apps/api/core", () => core);
vi.mock("@tauri-apps/plugin-fs", () => fs);
vi.mock("@tauri-apps/api/event", () => event);

const w = window as unknown as Record<string, unknown>;
const desktop = () => (w.__TAURI_INTERNALS__ = {});

afterEach(() => {
  core.invoke.mockReset();
  fs.stat.mockReset();
  fs.readFile.mockReset();
  event.listen.mockReset();
  delete w.__TAURI_INTERNALS__;
});

describe("asking the shell for the start-up file", () => {
  it("the browser build never asks: there is no shell and no such file", async () => {
    expect(await takeStartupFile()).toBe(null);
    expect(core.invoke).not.toHaveBeenCalled();
  });

  it("the desktop app asks the shell's start_file command, with nothing else", async () => {
    desktop();
    core.invoke.mockResolvedValueOnce("C:\\Users\\maker\\shelf.shz");
    expect(await takeStartupFile()).toBe("C:\\Users\\maker\\shelf.shz");
    expect(core.invoke).toHaveBeenCalledTimes(1);
    expect(core.invoke).toHaveBeenCalledWith(START_FILE);
    expect(START_FILE).toBe("start_file");
  });

  it("nothing from the shell is no file: null, an empty path, anything that is not a path", async () => {
    desktop();
    for (const answer of [null, undefined, "", 7, ["a.shz"], { path: "a.shz" }]) {
      core.invoke.mockResolvedValueOnce(answer);
      expect(await takeStartupFile()).toBe(null);
    }
  });

  it("a shell that refuses the command is passed on to the caller (App.tsx starts without a file)", async () => {
    desktop();
    core.invoke.mockRejectedValueOnce("start_file is not a command here");
    await expect(takeStartupFile()).rejects.toBe("start_file is not a command here");
  });
});

describe("the order at start", () => {
  it("with no start-up file nothing happens, as before", () => {
    expect(startupOrder(null, false)).toBe("nothing");
    expect(startupOrder(null, true)).toBe("nothing");
  });

  it("a start-up file opens at once unless unsaved work from the last run is offered back, when it waits for the answer", () => {
    expect(startupOrder("/home/maker/shelf.shz", false)).toBe("open");
    expect(startupOrder("/home/maker/shelf.shz", true)).toBe("wait");
  });

  it("names the file in what it says, by its name only", () => {
    expect(waitingNote("C:\\Users\\maker\\My Shelf.shz")).toBe("My Shelf.shz opens after you answer.");
    expect(notOpenedNote("/home/maker/étagère.shz")).toBe("étagère.shz is not open yet.");
    expect(openLabel("/home/maker/shelf.shz")).toBe("Open shelf.shz");
  });
});

describe("reading it, the way a dropped file is read", () => {
  const take = () => {
    const got: PickedFile[] = [];
    const refused: string[] = [];
    return { got, refused, cb: (f: PickedFile[]) => void got.push(...f), no: (m: string) => void refused.push(m) };
  };

  it("measures, then reads, then hands over the name, the bytes and the path to save back to", async () => {
    fs.stat.mockResolvedValue({ size: 3 });
    fs.readFile.mockResolvedValue(new Uint8Array([1, 2, 3]));
    const t = take();
    await takeTauriPaths(["C:\\Users\\maker\\shelf.shz"], t.cb, t.no);
    expect(fs.stat).toHaveBeenCalledWith("C:\\Users\\maker\\shelf.shz");
    expect(fs.readFile).toHaveBeenCalledWith("C:\\Users\\maker\\shelf.shz");
    expect(t.got).toEqual([{ name: "shelf.shz", bytes: new Uint8Array([1, 2, 3]), handle: { kind: "tauri", path: "C:\\Users\\maker\\shelf.shz" } }]);
    expect(t.refused).toEqual([]);
  });

  it("a file that cannot be read goes to the message with the reason, and is not handed over", async () => {
    fs.stat.mockRejectedValue("failed to open file at path: /gone.shz with error: No such file or directory (os error 2)");
    const t = take();
    await takeTauriPaths(["/gone.shz"], t.cb, t.no);
    expect(t.got).toEqual([]);
    expect(t.refused).toEqual(["failed to open file at path: /gone.shz with error: No such file or directory (os error 2)"]);
    expect(fs.readFile).not.toHaveBeenCalled();
  });

  it("a file over its limit is refused by its size, never read", async () => {
    fs.stat.mockResolvedValue({ size: MAX_READ_BYTES.shz + 1 });
    const t = take();
    await takeTauriPaths(["/big.shz"], t.cb, t.no);
    expect(t.refused).toEqual(["big.shz is 512 MB; Sawhorse reads documents of at most 512 MB"]);
    expect(fs.readFile).not.toHaveBeenCalled();
  });

  it("a name File > Open does not take is skipped without being read", async () => {
    const t = take();
    await takeTauriPaths(["/notes.txt", "/plugin.zip"], t.cb, t.no);
    expect(fs.stat).not.toHaveBeenCalled();
    expect(t.got).toEqual([]);
    expect(t.refused).toEqual([]);
  });

  it("a drop goes the same way: the same reads and the same message for the same file", async () => {
    desktop();
    let handler: ((e: { payload: string[] }) => unknown) | null = null;
    event.listen.mockImplementation(async (name, cb) => {
      expect(name).toBe("sawhorse://dropped");
      handler = cb;
      return () => {};
    });
    fs.stat.mockRejectedValue("forbidden path: /gone.shz");
    const dropped = take();
    await onTauriDrop(dropped.cb, dropped.no);
    await handler!({ payload: ["/gone.shz"] });
    const started = take();
    await takeTauriPaths(["/gone.shz"], started.cb, started.no);
    expect(started.refused).toEqual(dropped.refused);
    expect(dropped.refused).toEqual(["forbidden path: /gone.shz"]);
  });
});

describe("one list of what File > Open takes", () => {
  it("the shell's list (src-tauri/start-file) is the page's, in the same order", () => {
    const rust = readFileSync("src-tauri/start-file/src/lib.rs", "utf8");
    const m = /pub const EXTENSIONS: &\[&str\] = &\[([^\]]*)\];/.exec(rust);
    expect(m, "EXTENSIONS in src-tauri/start-file/src/lib.rs").not.toBeNull();
    const shell = [...m![1].matchAll(/"([^"]+)"/g)].map((x) => `.${x[1]}`);
    expect(shell).toEqual(ALL_EXTENSIONS);
  });
});
