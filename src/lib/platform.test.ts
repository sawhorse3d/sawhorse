import { afterEach, describe, expect, it, vi } from "vitest";
import { isMac, isMacControlClick } from "./platform";

// Lane 31 (#41): only a Mac is a Mac, so on Windows and Linux a Ctrl+click stays a Ctrl+click.
afterEach(() => {
  vi.restoreAllMocks();
});

function on(platform: string, userAgent = "Mozilla/5.0") {
  vi.spyOn(navigator, "platform", "get").mockReturnValue(platform);
  vi.spyOn(navigator, "userAgent", "get").mockReturnValue(userAgent);
}

const left = { button: 0, ctrlKey: true, metaKey: false };

describe("platform", () => {
  it("isMac: macOS by navigator.platform, or by the user agent when the platform is empty", () => {
    for (const [p, ua, mac] of [
      ["MacIntel", "", true],
      ["Win32", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)", false],
      ["Linux x86_64", "Mozilla/5.0 (X11; Linux x86_64)", false],
      ["Linux aarch64", "Mozilla/5.0 (X11; Linux aarch64)", false],
      ["iPhone", "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)", false],
      ["", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", true],
      ["", "Mozilla/5.0 (X11; Linux x86_64)", false],
    ] as const) {
      on(p, ua);
      expect(isMac(), `${p} | ${ua}`).toBe(mac);
      vi.restoreAllMocks();
    }
  });

  it("isMacControlClick: never on Windows or Linux, whatever the press", () => {
    for (const p of ["Win32", "Linux x86_64"]) {
      on(p, "Mozilla/5.0 (X11; Linux x86_64)");
      for (const e of [left, { ...left, pointerType: "mouse" }, { ...left, pointerType: "pen" }, { ...left, metaKey: true }, { ...left, button: 2 }, { ...left, ctrlKey: false }])
        expect(isMacControlClick(e), `${p} ${JSON.stringify(e)}`).toBe(false);
      vi.restoreAllMocks();
    }
  });

  it("isMacControlClick on a Mac: the left button with Control and not Cmd, from a mouse or a pen, not a touch", () => {
    on("MacIntel");
    expect(isMacControlClick(left)).toBe(true);
    expect(isMacControlClick({ ...left, pointerType: "mouse" })).toBe(true);
    expect(isMacControlClick({ ...left, pointerType: "pen" })).toBe(true);
    expect(isMacControlClick({ ...left, pointerType: "touch" })).toBe(false);
    expect(isMacControlClick({ ...left, metaKey: true })).toBe(false);
    expect(isMacControlClick({ ...left, ctrlKey: false })).toBe(false);
    expect(isMacControlClick({ ...left, button: 1 })).toBe(false);
    expect(isMacControlClick({ ...left, button: 2 })).toBe(false);
  });
});
