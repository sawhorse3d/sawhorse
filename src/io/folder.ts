// The folder the desktop's Open and Save dialogs start in (#71, #80).
//
// The dialogs were given no folder: Save a bare name ("Untitled.shz"), Open nothing. On
// Linux GTK's chooser then starts where it chooses: Save in the process's working
// directory, which for an AppImage is the mount it runs from, and Open in Recent.
//
// The page keeps the folder of the last file it opened or saved (FOLDER_KEY, kept between
// starts the way the units are) and names it to the shell before each dialog, after the
// folder of the model's own file for Save As. The shell's dialog_folder command
// (src-tauri/src/lib.rs, sawhorse_dialog_folder) answers with the first of those that is
// still a folder, then the Documents folder, then the home folder. It answers on Linux only:
// on Windows and macOS it answers nothing and the system's dialog starts where it starts,
// as before.
//
// Nothing here can stop a dialog from opening. A shell without the command, one that does
// not answer in ASK_MS, storage that cannot be read or written: each means no folder, and
// the dialog opens as it did.

/** localStorage key: the folder of the last file the desktop app opened or saved. */
export const FOLDER_KEY = "sawhorse.lastFolder";
/** The longest folder kept, in characters (the shell takes at most 4096 bytes). */
export const MAX_FOLDER = 1024;
/** How long the shell is waited for before the dialog opens without a folder. */
export const ASK_MS = 1500;

/**
 * The folder a file's path names: "/home/maker/models/shelf.shz" is in "/home/maker/models",
 * "/shelf.shz" in "/", "C:\\shelf.shz" in "C:\\". A bare name is in no folder (null).
 */
export function folderOf(path: string): string | null {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (i < 0) return null;
  if (i === 0) return path[0];
  const folder = path.slice(0, i);
  return /^[A-Za-z]:$/.test(folder) ? `${folder}${path[i]}` : folder;
}

/** `name` in `folder`, joined with the separator the folder is written with. */
export function inFolder(folder: string, name: string): string {
  if (folder.endsWith("/") || folder.endsWith("\\")) return `${folder}${name}`;
  return `${folder}${folder.includes("\\") && !folder.includes("/") ? "\\" : "/"}${name}`;
}

/** The folder kept, or null. */
export function keptFolder(): string | null {
  try {
    const v = localStorage.getItem(FOLDER_KEY);
    return v && v.length <= MAX_FOLDER ? v : null;
  } catch {
    return null;
  }
}

/** Keep the folder of a file just opened or saved (a full path; a bare name keeps nothing). */
export function rememberFolder(path: string): void {
  const folder = folderOf(path);
  if (!folder || folder.length > MAX_FOLDER) return;
  try {
    localStorage.setItem(FOLDER_KEY, folder);
  } catch {
    /* storage full or refused: the next dialog starts where it would have */
  }
}

/**
 * Desktop: the folder the next dialog starts in, or null for the system's own choice.
 * `beside` is the path of the model's own file (Save As starts in its folder). Never
 * throws and never waits longer than ASK_MS.
 */
export async function startFolder(beside?: string): Promise<string | null> {
  const folders = [...new Set([beside ? folderOf(beside) : null, keptFolder()].filter((f): f is string => !!f))];
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const late = new Promise<null>((r) => (timer = setTimeout(() => r(null), ASK_MS)));
    const answer = await Promise.race([invoke<string | null>("dialog_folder", { folders }), late]);
    return typeof answer === "string" && answer ? answer : null;
  } catch {
    return null;
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}
