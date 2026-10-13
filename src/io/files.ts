// Opening and saving files, the same way in every host:
//
// - Tauri (desktop): the dialog plugin picks a path, the fs plugin reads or writes it.
// - Browsers with the File System Access API (Chromium, Edge): native open/save pickers,
//   and Save writes back to the file that was opened or last saved.
// - Everything else, or `?io=basic` on the URL: a file input to open, a download to save.
//
// Every function returns null when the user cancels.
//
// Lane 16: a file is measured before it is read: one over its limit (MAX_READ_BYTES) is
// refused with FileTooLarge and never read into memory.

export interface FileType {
  description: string;
  mime: string;
  /** With the dot: ".stl". */
  extensions: string[];
}

/** Where a picked or saved file lives, so Save can write it again. */
export type FileHandle = { kind: "fsa"; handle: FsaFileHandle } | { kind: "tauri"; path: string };

export interface PickedFile {
  name: string;
  bytes: Uint8Array;
  handle?: FileHandle;
}

/** Largest file read whole (lane 16): a .shz (the kernel reads at most 1 GB from inside
 * one), an import (the kernel's readers take 256 MB), a plugin package (.zip). */
export const MAX_READ_BYTES = { shz: 512 * 1024 * 1024, import: 256 * 1024 * 1024, zip: 64 * 1024 * 1024 };

/** A file over the size Sawhorse reads. */
export class FileTooLarge extends Error {
  constructor(name: string, size: number, limit: number) {
    super(`${name} is ${Math.round(size / 1024 / 1024)} MB; Sawhorse reads ${extensionOf(name) === ".shz" ? "documents" : extensionOf(name) === ".zip" ? "plugin packages" : "imports"} of at most ${Math.round(limit / 1024 / 1024)} MB`);
    this.name = "FileTooLarge";
  }
}

/** The most bytes Sawhorse reads for a file of this name. */
export function readLimit(name: string): number {
  const ext = extensionOf(name);
  return ext === ".shz" ? MAX_READ_BYTES.shz : ext === ".zip" ? MAX_READ_BYTES.zip : MAX_READ_BYTES.import;
}

/** Throw FileTooLarge when `size` is over the limit for `name`. */
export function checkSize(name: string, size: number): void {
  const limit = readLimit(name);
  if (size > limit) throw new FileTooLarge(baseName(name), size, limit);
}

/** Desktop: a file's size (stat), checked before it is read, then its bytes. */
async function readTauriFile(path: string): Promise<Uint8Array> {
  const { readFile, stat } = await import("@tauri-apps/plugin-fs");
  checkSize(path, (await stat(path)).size);
  return readFile(path);
}

export const SHZ: FileType = { description: "Sawhorse document", mime: "application/vnd.sawhorse.shz", extensions: [".shz"] };

export const IMPORT_TYPES: FileType[] = [
  { description: "SketchUp", mime: "application/vnd.sketchup.skp", extensions: [".skp"] },
  { description: "STL", mime: "model/stl", extensions: [".stl"] },
  { description: "3MF", mime: "model/3mf", extensions: [".3mf"] },
  { description: "glTF", mime: "model/gltf-binary", extensions: [".glb", ".gltf"] },
  { description: "Wavefront OBJ", mime: "model/obj", extensions: [".obj"] },
  { description: "DXF", mime: "image/vnd.dxf", extensions: [".dxf"] },
  { description: "SVG", mime: "image/svg+xml", extensions: [".svg"] },
];

/** Every extension Sawhorse opens or imports (for `accept` and drag-and-drop). */
export const ALL_EXTENSIONS = [...SHZ.extensions, ...IMPORT_TYPES.flatMap((t) => t.extensions)];

export function extensionOf(name: string): string {
  const m = /\.([^./\\]+)$/.exec(name);
  return m ? `.${m[1].toLowerCase()}` : "";
}

export function baseName(path: string): string {
  return path.replace(/^.*[\\/]/, "");
}

/** The name without its extension. */
export function stem(name: string): string {
  return baseName(name).replace(/\.[^.]*$/, "") || "Untitled";
}

// ---- host detection -------------------------------------------------------------------

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

interface FsaWritable {
  write(data: BufferSource | Blob): Promise<void>;
  close(): Promise<void>;
}
export interface FsaFileHandle {
  name: string;
  getFile(): Promise<File>;
  createWritable(): Promise<FsaWritable>;
}
interface FsaWindow {
  showOpenFilePicker(o: { multiple?: boolean; types?: { description: string; accept: Record<string, string[]> }[] }): Promise<FsaFileHandle[]>;
  showSaveFilePicker(o: { suggestedName?: string; types?: { description: string; accept: Record<string, string[]> }[] }): Promise<FsaFileHandle>;
}

/** The File System Access API, unless `?io=basic` asks for the input-and-download path. */
function fsa(): FsaWindow | null {
  if (typeof window === "undefined") return null;
  try {
    if (new URLSearchParams(window.location.search).get("io") === "basic") return null;
  } catch {
    /* no URL: fine */
  }
  const w = window as unknown as Partial<FsaWindow>;
  return typeof w.showOpenFilePicker === "function" && typeof w.showSaveFilePicker === "function" ? (w as FsaWindow) : null;
}

const fsaTypes = (types: FileType[]) => types.map((t) => ({ description: t.description, accept: { [t.mime]: t.extensions } }));
const isAbort = (e: unknown) => e instanceof DOMException && e.name === "AbortError";

// ---- open -----------------------------------------------------------------------------

/** Ask for one file of `types`. */
export async function pickFile(types: FileType[]): Promise<PickedFile | null> {
  if (isTauri()) {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const path = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Supported files", extensions: types.flatMap((t) => t.extensions.map((e) => e.slice(1))) }],
    });
    if (typeof path !== "string") return null;
    return { name: baseName(path), bytes: await readTauriFile(path), handle: { kind: "tauri", path } };
  }
  const api = fsa();
  if (api) {
    try {
      const [h] = await api.showOpenFilePicker({ multiple: false, types: [{ description: "Supported files", accept: { "application/octet-stream": types.flatMap((t) => t.extensions) } }] });
      const f = await h.getFile();
      checkSize(f.name, f.size);
      return { name: f.name, bytes: new Uint8Array(await f.arrayBuffer()), handle: { kind: "fsa", handle: h } };
    } catch (e) {
      if (isAbort(e)) return null;
      if (e instanceof FileTooLarge) throw e;
      // A picker that will not open here (an iframe, a policy) falls back to the input.
    }
  }
  return pickWithInput(types);
}

function pickWithInput(types: FileType[]): Promise<PickedFile | null> {
  return new Promise((resolve, reject) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = types.flatMap((t) => t.extensions).join(",");
    input.style.display = "none";
    let settled = false;
    const done = (v: PickedFile | null) => {
      if (settled) return;
      settled = true;
      input.remove();
      resolve(v);
    };
    input.addEventListener("change", async () => {
      const f = input.files?.[0];
      try {
        done(f ? await readBrowserFile(f) : null);
      } catch (e) {
        settled = true;
        input.remove();
        reject(e);
      }
    });
    input.addEventListener("cancel", () => done(null));
    document.body.appendChild(input);
    input.click();
  });
}

/** A browser File's bytes, refused before reading when it is over its limit. */
export async function readBrowserFile(f: File): Promise<PickedFile> {
  checkSize(f.name, f.size);
  return { name: f.name, bytes: new Uint8Array(await f.arrayBuffer()) };
}

// ---- save -----------------------------------------------------------------------------

/**
 * Save `bytes` as `name`. With `handle` (Save, not Save As) writes back to that file
 * without asking. Returns where it went, or null when cancelled.
 */
export async function saveFile(name: string, bytes: Uint8Array, type: FileType, handle?: FileHandle): Promise<{ name: string; handle?: FileHandle } | null> {
  if (isTauri()) {
    let path = handle?.kind === "tauri" ? handle.path : null;
    if (!path) {
      const { save } = await import("@tauri-apps/plugin-dialog");
      path = await save({ defaultPath: name, filters: [{ name: type.description, extensions: type.extensions.map((e) => e.slice(1)) }] });
      if (!path) return null;
    }
    // Lane 16: written beside, synced, read back, then renamed over the file (the previous
    // one kept as .bak), never truncated in place (src-tauri/safe-save).
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("save_file_atomic", bytes, { headers: { "x-path": encodeURIComponent(path) } });
    return { name: baseName(path), handle: { kind: "tauri", path } };
  }
  const api = fsa();
  if (api) {
    try {
      const h = handle?.kind === "fsa" ? handle.handle : await api.showSaveFilePicker({ suggestedName: name, types: fsaTypes([type]) });
      const w = await h.createWritable();
      await w.write(bytes as BufferSource);
      await w.close();
      return { name: h.name, handle: { kind: "fsa", handle: h } };
    } catch (e) {
      if (isAbort(e)) return null;
      // Not allowed here: fall back to a download.
    }
  }
  download(name, bytes, type.mime);
  return { name };
}

/** Hand the browser a file to save (its downloads folder, or its own Save dialog). */
export function download(name: string, bytes: Uint8Array, mime: string): void {
  const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: mime }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.style.display = "none";
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 30_000);
}

// ---- drag and drop in Tauri -------------------------------------------------------------

/**
 * In the desktop app the OS drag-and-drop reaches the Rust side, which allows the dropped
 * paths in the fs scope and emits `sawhorse://dropped`; read them here. Browser builds
 * use HTML drag-and-drop instead (the app root handles `drop`).
 */
export async function onTauriDrop(cb: (files: PickedFile[]) => Promise<void> | void, refused?: (message: string) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<string[]>("sawhorse://dropped", async (e) => {
    // One at a time (lane 16): each file is measured, read and taken in before the next is
    // read, so dropping several large files never holds them all at once.
    for (const path of e.payload) {
      if (!ALL_EXTENSIONS.includes(extensionOf(path))) continue;
      try {
        await cb([{ name: baseName(path), bytes: await readTauriFile(path), handle: { kind: "tauri", path } }]);
      } catch (err) {
        refused?.(err instanceof Error ? err.message : String(err));
      }
    }
  });
}
