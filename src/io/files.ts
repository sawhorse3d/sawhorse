// Opening and saving files, the same way in every host:
//
// - Tauri (desktop): the dialog plugin picks a path, the fs plugin reads or writes it.
// - Browsers with the File System Access API (Chromium, Edge): native open/save pickers,
//   and Save writes back to the file that was opened or last saved.
// - Everything else, or `?io=basic` on the URL: a file input to open, a download to save.
//
// Every function returns null when the user cancels.

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
    const { readFile } = await import("@tauri-apps/plugin-fs");
    return { name: baseName(path), bytes: await readFile(path), handle: { kind: "tauri", path } };
  }
  const api = fsa();
  if (api) {
    try {
      const [h] = await api.showOpenFilePicker({ multiple: false, types: [{ description: "Supported files", accept: { "application/octet-stream": types.flatMap((t) => t.extensions) } }] });
      const f = await h.getFile();
      return { name: f.name, bytes: new Uint8Array(await f.arrayBuffer()), handle: { kind: "fsa", handle: h } };
    } catch (e) {
      if (isAbort(e)) return null;
      // A picker that will not open here (an iframe, a policy) falls back to the input.
    }
  }
  return pickWithInput(types);
}

function pickWithInput(types: FileType[]): Promise<PickedFile | null> {
  return new Promise((resolve) => {
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
      done(f ? await readBrowserFile(f) : null);
    });
    input.addEventListener("cancel", () => done(null));
    document.body.appendChild(input);
    input.click();
  });
}

export async function readBrowserFile(f: File): Promise<PickedFile> {
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
    const { writeFile } = await import("@tauri-apps/plugin-fs");
    await writeFile(path, bytes);
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
export async function onTauriDrop(cb: (files: PickedFile[]) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  const { readFile } = await import("@tauri-apps/plugin-fs");
  return listen<string[]>("sawhorse://dropped", async (e) => {
    const files: PickedFile[] = [];
    for (const path of e.payload) {
      if (!ALL_EXTENSIONS.includes(extensionOf(path))) continue;
      files.push({ name: baseName(path), bytes: await readFile(path), handle: { kind: "tauri", path } });
    }
    if (files.length) cb(files);
  });
}
