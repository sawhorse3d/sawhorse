// The file the desktop app was started with (lane 37, #45).
//
// Before lane 37 a double-click on a .shz in Windows Explorer could not start Sawhorse: the
// installers declared no file association and the shell never looked at its arguments. Now
// Windows starts Sawhorse with the file's path, and so does a path after the program's name
// on any system. The shell (src-tauri/src/lib.rs, src-tauri/start-file) picks the one file to
// open from its arguments; it does not send it to the page, which is not listening yet when
// the shell starts. The page asks instead, once it is ready to open a file: the shell's
// `start_file` command answers with the path the first time and with nothing after that, so a
// reload of the page never opens it again. The file is then read and opened exactly as a file
// dropped on the window is (takeTauriPaths in src/io/files.ts): the same checks, the same
// messages.
//
// The autosave comes first (App.tsx). At start the app sets aside what the last run kept
// for "Restore unsaved work?" (takeAutosave in src/lib/autosave.ts) and only then asks for
// the start-up file. Opening a file deletes this session's autosave (it is the copy to go
// back to), so a file opened before the last run's work was set aside would delete that
// work. And when there is work to offer, the file waits for the answer: Restore brings the
// work back into a new untitled model and the file is offered beside it ("Open shelf.shz"),
// Discard deletes the work and opens the file. Restoring after the file had opened would put
// the work in the file's place under the file's name, where Save would write it over the file.
import { baseName, isTauri } from "./files";

/** The shell's command that hands the start-up file over, once. */
export const START_FILE = "start_file";

/**
 * The path of the file the desktop app was started with, the first time it is asked (null
 * every time after that, and always in the browser build, which has no such file and never
 * calls the shell).
 */
export async function takeStartupFile(): Promise<string | null> {
  if (!isTauri()) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  const path = await invoke<unknown>(START_FILE);
  return typeof path === "string" && path !== "" ? path : null;
}

/** What the page does with a start-up file, given whether "Restore unsaved work?" is up. */
export function startupOrder(path: string | null, offerPending: boolean): "nothing" | "open" | "wait" {
  if (path === null) return "nothing";
  return offerPending ? "wait" : "open";
}

/** Said after the restore offer while a start-up file waits for its answer. */
export function waitingNote(path: string): string {
  return `${baseName(path)} opens after you answer.`;
}

/** Said after Restore, beside the button that opens the start-up file. */
export function notOpenedNote(path: string): string {
  return `${baseName(path)} is not open yet.`;
}

/** The label of the button that opens the start-up file after Restore. */
export function openLabel(path: string): string {
  return `Open ${baseName(path)}`;
}
