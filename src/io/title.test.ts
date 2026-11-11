// Lane 39: the desktop window's title bar shows the page's title, the model's name. What the
// shell makes of the text (control characters, length, blank) is tested in
// src-tauri/window-title (cargo test -p sawhorse-window-title); this is the page's side.
import { readFileSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import { showTitle, WINDOW_TITLE } from "./title";

const core = vi.hoisted(() => ({ invoke: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>() }));
vi.mock("@tauri-apps/api/core", () => core);

const w = window as unknown as Record<string, unknown>;
const desktop = () => (w.__TAURI_INTERNALS__ = {});

afterEach(() => {
  core.invoke.mockReset();
  delete w.__TAURI_INTERNALS__;
  document.title = "";
});

describe("the window's title", () => {
  it("the browser build sets the page's title and never calls the shell", async () => {
    await showTitle("• shelf - Sawhorse");
    expect(document.title).toBe("• shelf - Sawhorse");
    await showTitle("Untitled - Sawhorse");
    expect(document.title).toBe("Untitled - Sawhorse");
    expect(core.invoke).not.toHaveBeenCalled();
  });

  it("the desktop app hands the shell exactly the text the page's title has, with nothing else", async () => {
    desktop();
    core.invoke.mockResolvedValue(null);
    for (const text of ["Untitled - Sawhorse", "• Untitled - Sawhorse", "Regal für Jörg - Sawhorse", "• 本棚 - Sawhorse"]) {
      await showTitle(text);
      expect(document.title).toBe(text);
      expect(core.invoke).toHaveBeenLastCalledWith(WINDOW_TITLE, { title: document.title });
    }
    expect(core.invoke).toHaveBeenCalledTimes(4);
    expect(WINDOW_TITLE).toBe("window_title");
  });

  it("a refused or failing call is swallowed: nothing rejects and the page's title is kept", async () => {
    desktop();
    core.invoke.mockRejectedValueOnce("window_title is not a command here");
    await expect(showTitle("shelf - Sawhorse")).resolves.toBeUndefined();
    expect(document.title).toBe("shelf - Sawhorse");
    core.invoke.mockRejectedValueOnce(new Error("the window is gone"));
    await expect(showTitle("• shelf - Sawhorse")).resolves.toBeUndefined();
    expect(document.title).toBe("• shelf - Sawhorse");
    core.invoke.mockImplementationOnce(() => {
      throw new TypeError("the bridge is not there");
    });
    await expect(showTitle("Untitled - Sawhorse")).resolves.toBeUndefined();
    expect(document.title).toBe("Untitled - Sawhorse");
    expect(core.invoke).toHaveBeenCalledTimes(3);
  });

  it("the page's title is set before the shell answers, even when it never does", async () => {
    desktop();
    core.invoke.mockReturnValueOnce(new Promise(() => {}));
    void showTitle("• shelf - Sawhorse");
    expect(document.title).toBe("• shelf - Sawhorse");
  });

  it("App.tsx gives its title through showTitle, not to document.title directly", () => {
    const app = readFileSync("src/App.tsx", "utf8");
    expect(app).toMatch(/showTitle\(`\$\{dirty \? "• " : ""\}/);
    expect(app).not.toMatch(/document\.title\s*=/);
  });

  it("the shell has the command the page calls", () => {
    const lib = readFileSync("src-tauri/src/lib.rs", "utf8");
    expect(lib).toMatch(new RegExp(`fn ${WINDOW_TITLE}\\(window: tauri::Window, title: String\\)`));
    const handlers = /generate_handler!\[([^\]]*)\]/.exec(lib);
    expect(handlers?.[1].split(",").map((s) => s.trim())).toContain(WINDOW_TITLE);
  });
});
