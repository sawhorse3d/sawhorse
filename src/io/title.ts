// The window's title (lane 39).
//
// The page names the model in its title: a dot when there are unsaved changes, the model's
// name or "Untitled", then " - Sawhorse" (App.tsx). A browser shows that on the tab. Before
// lane 39 the desktop app's title bar said only "Sawhorse", whatever model was open, and
// since lane 37 a second double-click on a .shz opens a second window, so two windows could
// not be told apart. Now, in the desktop app, the page also hands the same text to the
// shell's `window_title` command (src-tauri/src/lib.rs), which cleans it
// (src-tauri/window-title: no control characters or line breaks, at most 200 characters,
// "Sawhorse" when nothing is left) and sets it on this window only. The browser build never
// calls the shell. A call that fails leaves the title bar as it was and is never shown: the
// title is a convenience, and the page's own title is already right.
import { isTauri } from "./files";

/** The shell's command that sets the window's title. */
export const WINDOW_TITLE = "window_title";

/**
 * Give the page the title `text`, and in the desktop app the window's title bar too.
 * Resolves once the shell has answered; never rejects.
 */
export async function showTitle(text: string): Promise<void> {
  document.title = text;
  if (!isTauri()) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke(WINDOW_TITLE, { title: text });
  } catch {
    // The title bar keeps the text it had; nothing to tell the person.
  }
}
