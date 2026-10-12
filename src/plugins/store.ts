// What the plugin system keeps between sessions and inside documents:
//
// - which plugins are enabled, and the permissions the person granted each, bound to the
//   package they enabled by its SHA-256 (localStorage, per device; a plugin that asks for
//   more after an update, or whose files are not the ones enabled, is off until granted
//   again);
// - each plugin's app-scoped store (localStorage, 256 KB per plugin);
// - each plugin's document-scoped store: in memory for the open document, written into the
//   .shz as `plugin-data.json` on save and read back on open (readers ignore entries they
//   do not know, docs/format.md), cleared by New;
// - packages installed from a .zip in the browser build (IndexedDB). The desktop build keeps
//   them as folders in app data (platform.ts).
//
// Every storage access is wrapped: a blocked or full store degrades to "nothing kept".
import { strFromU8, strToU8, unzipSync, zipSync } from "fflate";
import type { Permission } from "../../packages/plugin-api/src/manifest.ts";
import { sha256Hex } from "../lib/sha256";

const ENABLED_KEY = "sawhorse.plugins.enabled";
const KV_PREFIX = "sawhorse.plugin.";
export const APP_QUOTA = 256 * 1024;
export const DOC_QUOTA = 64 * 1024;
export const DOC_ENTRY = "plugin-data.json";

export interface Grant {
  version: string;
  permissions: Permission[];
  /**
   * SHA-256 of the package the person enabled (packageDigest). A grant is for that code, not
   * for an id: a different package with the same id (a replacement zip, an update, a zip
   * claiming a bundled sample's id) is off until the person enables it again.
   */
  digest?: string;
  /** Where the enabled package came from. */
  source?: "bundled" | "installed" | "unpacked" | "session";
  /** The folder of an unpacked plugin (desktop). */
  location?: string;
}

/**
 * A digest of every file of a plugin package (paths and contents, in path order), so any
 * change to its code, manifest or other files changes it.
 */
export function packageDigest(files: Record<string, string>): string {
  const parts: string[] = [];
  for (const path of Object.keys(files).sort()) parts.push(`${path.length}:${path}${files[path].length}:${files[path]}`);
  return sha256Hex(parts.join(""));
}

/**
 * Whether a grant was given to this package. The same digest always matches. Two cases
 * keep a grant across a change of code, because the person chose the source and not a
 * particular build of it: a bundled sample, updated with the app (it is the app's own
 * code), and an unpacked folder on the desktop, re-read on Reload while its author edits
 * it (the grant remembers the folder). A grant from before digests existed holds only for
 * a bundled sample.
 */
export function grantMatches(g: Grant | undefined, pkg: { digest: string; source: Grant["source"]; location?: string }): boolean {
  if (!g) return false;
  if (g.digest && g.digest === pkg.digest) return true;
  if (pkg.source === "bundled" && (g.source === "bundled" || (!g.source && !g.digest))) return true;
  if (pkg.source === "unpacked" && g.source === "unpacked" && !!g.location && g.location === pkg.location) return true;
  return false;
}

function readJson<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v ? (JSON.parse(v) as T) : fallback;
  } catch {
    return fallback;
  }
}

function writeJson(key: string, v: unknown): boolean {
  try {
    localStorage.setItem(key, JSON.stringify(v));
    return true;
  } catch {
    return false;
  }
}

/** Plugins enabled on this device, with what was granted. */
export function loadGrants(): Record<string, Grant> {
  const g = readJson<Record<string, Grant>>(ENABLED_KEY, {});
  return g && typeof g === "object" && !Array.isArray(g) ? g : {};
}

export function saveGrants(g: Record<string, Grant>): void {
  writeJson(ENABLED_KEY, g);
}

/** Whether a grant still covers what the manifest asks for. */
export function grantCovers(g: Grant | undefined, permissions: Permission[]): boolean {
  return !!g && Array.isArray(g.permissions) && permissions.every((p) => g.permissions.includes(p));
}

const checkKey = (key: unknown): string => {
  if (typeof key !== "string" || key.length < 1 || key.length > 200) throw new TypeError("a storage key is a string of 1 to 200 characters");
  return key;
};

function sizeOf(v: unknown): number {
  return JSON.stringify(v ?? null).length;
}

/** A plugin's app-scoped store (localStorage). */
export class AppStore {
  private key: string;
  constructor(pluginId: string) {
    this.key = `${KV_PREFIX}${pluginId}`;
  }
  private all(): Record<string, unknown> {
    const v = readJson<Record<string, unknown>>(this.key, {});
    return v && typeof v === "object" && !Array.isArray(v) ? v : {};
  }
  get(key: unknown): unknown {
    return this.all()[checkKey(key)] ?? null;
  }
  set(key: unknown, value: unknown): void {
    const all = this.all();
    all[checkKey(key)] = value;
    if (sizeOf(all) > APP_QUOTA) throw new RangeError(`the plugin's store would be over ${APP_QUOTA / 1024} KB`);
    if (!writeJson(this.key, all)) throw new Error("this browser is not keeping data for the app (private window or storage blocked)");
  }
  delete(key: unknown): void {
    const all = this.all();
    delete all[checkKey(key)];
    writeJson(this.key, all);
  }
  keys(): string[] {
    return Object.keys(this.all());
  }
  clear(): void {
    try {
      localStorage.removeItem(this.key);
    } catch {
      /* nothing kept anyway */
    }
  }
}

/** Every plugin's document-scoped store for the open document. */
export class DocStore {
  private data: Record<string, Record<string, unknown>> = {};

  get(plugin: string, key: unknown): unknown {
    return this.data[plugin]?.[checkKey(key)] ?? null;
  }
  set(plugin: string, key: unknown, value: unknown): void {
    const next = { ...(this.data[plugin] ?? {}), [checkKey(key)]: value };
    if (sizeOf(next) > DOC_QUOTA) throw new RangeError(`the plugin's document store would be over ${DOC_QUOTA / 1024} KB`);
    this.data[plugin] = next;
  }
  delete(plugin: string, key: unknown): void {
    if (this.data[plugin]) delete this.data[plugin][checkKey(key)];
  }
  keys(plugin: string): string[] {
    return Object.keys(this.data[plugin] ?? {});
  }
  /** A new document: nothing stored. */
  clear(): void {
    this.data = {};
  }
  snapshot(): Record<string, Record<string, unknown>> {
    return JSON.parse(JSON.stringify(this.data)) as Record<string, Record<string, unknown>>;
  }
  load(data: unknown): void {
    this.data = {};
    if (!data || typeof data !== "object" || Array.isArray(data)) return;
    for (const [k, v] of Object.entries(data as Record<string, unknown>)) if (v && typeof v === "object" && !Array.isArray(v) && sizeOf(v) <= DOC_QUOTA) this.data[k] = v as Record<string, unknown>;
  }
  empty(): boolean {
    return Object.values(this.data).every((v) => Object.keys(v).length === 0);
  }

  /** The .shz with this store added as plugin-data.json (unchanged when nothing is stored). */
  attach(shz: Uint8Array): Uint8Array {
    if (this.empty()) return shz;
    const entries = unzipSync(shz);
    entries[DOC_ENTRY] = strToU8(JSON.stringify({ version: 1, plugins: this.snapshot() }));
    return zipSync(entries);
  }

  /** Read plugin-data.json from an opened .shz (none: an empty store). */
  extract(shz: Uint8Array): void {
    try {
      const entries = unzipSync(shz, { filter: (f) => f.name === DOC_ENTRY });
      const raw = entries[DOC_ENTRY];
      this.load(raw ? (JSON.parse(strFromU8(raw)) as { plugins?: unknown }).plugins : null);
    } catch {
      this.clear();
    }
  }
}

// ---- installed packages in the browser (IndexedDB) ------------------------------------------

const DB = "sawhorse-plugins";
const STORE = "packages";

function openDb(): Promise<IDBDatabase | null> {
  return new Promise((resolve) => {
    try {
      if (typeof indexedDB === "undefined") return resolve(null);
      const req = indexedDB.open(DB, 1);
      req.onupgradeneeded = () => req.result.createObjectStore(STORE);
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => resolve(null);
      req.onblocked = () => resolve(null);
    } catch {
      resolve(null);
    }
  });
}

async function tx<T>(mode: IDBTransactionMode, run: (s: IDBObjectStore) => IDBRequest<T>): Promise<T | null> {
  const db = await openDb();
  if (!db) return null;
  return new Promise((resolve) => {
    try {
      const r = run(db.transaction(STORE, mode).objectStore(STORE));
      r.onsuccess = () => resolve(r.result);
      r.onerror = () => resolve(null);
    } catch {
      resolve(null);
    }
  });
}

export const browserPackages = {
  async list(): Promise<Record<string, string>[]> {
    return ((await tx("readonly", (s) => s.getAll())) ?? []) as Record<string, string>[];
  },
  async put(id: string, files: Record<string, string>): Promise<boolean> {
    return (await tx("readwrite", (s) => s.put(files, id))) !== null;
  },
  async remove(id: string): Promise<void> {
    await tx("readwrite", (s) => s.delete(id));
  },
};
