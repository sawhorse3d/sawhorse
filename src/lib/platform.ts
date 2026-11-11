// Which platform the app runs on, in one place: the checks that make it behave the way that
// platform's own apps do.

/** Running on macOS (the desktop app or a browser). */
export function isMac(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Mac/.test(navigator.platform || navigator.userAgent || "");
}

/** Running on Linux (the desktop app or a browser): navigator.platform, else the user agent. */
function isLinux(): boolean {
  if (typeof navigator === "undefined" || isMac()) return false;
  return /Linux|X11/.test(navigator.platform || navigator.userAgent || "");
}

/**
 * The desktop app's last File menu item (lane 35, #44), named as each platform's own apps
 * name it: "Exit" on Windows, "Quit Sawhorse" on macOS, "Quit" on Linux.
 */
export function quitLabel(): string {
  if (isMac()) return "Quit Sawhorse";
  return isLinux() ? "Quit" : "Exit";
}

/**
 * A press macOS means as the right click: the left button with Control held (not Cmd). On a
 * Mac, Control+click is the secondary click everywhere, and for a one-button mouse, a
 * trackpad, or a left hand on a Magic Mouse it is the right click that comes easily. The
 * page sees a left press with `ctrlKey`; the app treats it as the right button (see
 * `leftButtonNav` in src/tools/navigate.ts). Cmd stays the tools' Ctrl there.
 */
export function isMacControlClick(e: { button: number; ctrlKey: boolean; metaKey: boolean; pointerType?: string }): boolean {
  return e.button === 0 && e.ctrlKey && !e.metaKey && e.pointerType !== "touch" && isMac();
}
