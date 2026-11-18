import { readFileSync } from "node:fs";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CHANGELOG_URL, GUIDE_URL, helpLinks, openLink, type HelpLink } from "./help";

// Lane 67 (#97): the Help menu in each build, and how its links open.

const links = (list: HelpLink[]) => list.map((h) => (h === "-" ? "-" : [h.label, h.url]));

afterEach(() => {
  vi.unstubAllEnvs();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe("helpLinks", () => {
  it("the desktop app: User Guide, What's New and Source on GitHub, on the public site and GitHub", () => {
    expect(links(helpLinks(true))).toEqual([
      ["User Guide", "https://sawhorse3d.com/guide/"],
      ["What's New", "https://sawhorse3d.com/changelog/"],
      "-",
      ["Source on GitHub", "https://github.com/sawhorse3d/sawhorse"],
    ]);
  });

  it("the desktop app's pages stay on the public site whatever base the build was made with", () => {
    vi.stubEnv("BASE_URL", "/app/");
    expect(links(helpLinks(true))).toEqual([
      ["User Guide", "https://sawhorse3d.com/guide/"],
      ["What's New", "https://sawhorse3d.com/changelog/"],
      "-",
      ["Source on GitHub", "https://github.com/sawhorse3d/sawhorse"],
    ]);
    expect(GUIDE_URL).toBe("https://sawhorse3d.com/guide/");
    expect(CHANGELOG_URL).toBe("https://sawhorse3d.com/changelog/");
  });

  it("the browser demo (base /app/): the four entries, the site's pages beside the demo", () => {
    vi.stubEnv("BASE_URL", "/app/");
    expect(links(helpLinks(false, true))).toEqual([
      ["User Guide", "/guide/"],
      ["What's New", "/changelog/"],
      "-",
      ["Download the Desktop App", "https://github.com/sawhorse3d/sawhorse/releases/latest"],
      ["Source on GitHub", "https://github.com/sawhorse3d/sawhorse"],
    ]);
    const download = helpLinks(false, true).find((h) => h !== "-" && h.label === "Download the Desktop App");
    expect(download !== "-" && download?.title).toBe("Windows, macOS and Linux installers on GitHub");
  });

  it("the github.io address (base /sawhorse/app/): the site's pages under /sawhorse/", () => {
    vi.stubEnv("BASE_URL", "/sawhorse/app/");
    expect(links(helpLinks(false, true)).slice(0, 2)).toEqual([
      ["User Guide", "/sawhorse/guide/"],
      ["What's New", "/sawhorse/changelog/"],
    ]);
  });

  for (const base of ["./", "/"])
    it(`a browser build that is not the demo (base ${base}, as npm run preview and npm run dev): the four entries, on the public site`, () => {
      vi.stubEnv("BASE_URL", base);
      expect(links(helpLinks(false, false))).toEqual([
        ["User Guide", "https://sawhorse3d.com/guide/"],
        ["What's New", "https://sawhorse3d.com/changelog/"],
        "-",
        ["Download the Desktop App", "https://github.com/sawhorse3d/sawhorse/releases/latest"],
        ["Source on GitHub", "https://github.com/sawhorse3d/sawhorse"],
      ]);
    });

  it("which build it is comes from the build: the demo flag, never in the desktop app", () => {
    vi.stubEnv("BASE_URL", "/app/");
    vi.stubEnv("VITE_WEB_DEMO", "1");
    expect(links(helpLinks(false))[0]).toEqual(["User Guide", "/guide/"]);
    vi.stubEnv("VITE_WEB_DEMO", "");
    expect(links(helpLinks(false))[0]).toEqual(["User Guide", "https://sawhorse3d.com/guide/"]);
  });

  it("every page the desktop menu opens is one the shell opens, and the shell opens no other help page", () => {
    // src-tauri/device-key/src/lib.rs HELP_URLS: open_external refuses every other address.
    const rust = readFileSync(path.join(__dirname, "..", "..", "src-tauri", "device-key", "src", "lib.rs"), "utf8");
    const block = /pub const HELP_URLS: \[&str; (\d+)\] = \[([^\]]*)\];/.exec(rust);
    expect(block).not.toBeNull();
    const shell = [...block![2].matchAll(/"([^"]*)"/g)].map((m) => m[1]);
    expect(shell.length).toBe(Number(block![1]));
    const desktop = helpLinks(true).flatMap((h) => (h === "-" ? [] : [h.url]));
    expect(desktop).toEqual(shell);
  });
});

describe("openLink", () => {
  it("in the browser: a new tab, without a way back to this page", () => {
    const open = vi.fn(() => null);
    vi.stubGlobal("open", open);
    const failed = vi.fn();
    openLink("/guide/", failed);
    expect(open).toHaveBeenCalledTimes(1);
    expect(open).toHaveBeenCalledWith("/guide/", "_blank", "noopener,noreferrer");
    expect(failed).not.toHaveBeenCalled();
  });

  it("in the desktop app: the shell's open_external with the address, and no window.open", async () => {
    const open = vi.fn(() => null);
    vi.stubGlobal("open", open);
    const invoke = vi.fn(async () => null);
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = { invoke };
    const failed = vi.fn();
    openLink(GUIDE_URL, failed);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
    expect(invoke).toHaveBeenCalledWith("open_external", { url: "https://sawhorse3d.com/guide/" });
    expect(open).not.toHaveBeenCalled();
    await Promise.resolve();
    expect(failed).not.toHaveBeenCalled();
  });

  it("a shell that refuses: the address is said, so it can be typed", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
      invoke: vi.fn(async () => {
        throw "refusing to open https://sawhorse3d.com/guide/: only checkout pages open from the app";
      }),
    };
    const failed = vi.fn();
    openLink(GUIDE_URL, failed);
    await vi.waitFor(() => expect(failed).toHaveBeenCalledTimes(1));
    expect(failed.mock.calls[0][0]).toBe(
      "Could not open https://sawhorse3d.com/guide/ in the browser: refusing to open https://sawhorse3d.com/guide/: only checkout pages open from the app",
    );
  });
});
