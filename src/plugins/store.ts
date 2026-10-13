// What the plugin system keeps between sessions and inside documents:
//
// - which plugins are enabled, and the permissions the person granted each, bound to the
//   package they enabled by its SHA-256 (localStorage, per device; a plugin that asks for
//   more after an update, or whose files are not the ones enabled, is off until granted
//   again);
// - each plugin's app-scoped store (localStorage, 256 KB per plugin);
// - each plugin's document-scoped store: in memory for the open document, written into the
//   .shz as `plugin-data.json` on save and read back on open (readers ignore entries they
//   do not know, docs/format.md), cleared by New. Lane 16: the kernel writes and reads that
//   entry (capped) and hands the text over; the app never unzips a user's file;
// - packages installed from a .zip in the browser build (IndexedDB). The desktop build keeps
//   them as folders in app data (platform.ts).
//
// Every storage access is wrapped: a blocked or full store degrades to "nothing kept".
import type { Permission } from "../../packages/plugin-api/src/manifest.ts";
import { sha256Hex } from "../lib/sha256";

const ENABLED_KEY = "sawhorse.plugins.enabled";
const KV_PREFIX = "sawhorse.plugin.";
export const APP_QUOTA = 256 * 1024;
export const DOC_QUOTA = 64 * 1024;
/** Lane 16: every plugin's app-scoped stores together (characters of JSON in localStorage). */
export const APP_TOTAL = 1024 * 1024;
/** Lane 16: every plugin's document-scoped stores together (plugin-data.json's own cap). */
export const DOC_TOTAL = 1024 * 1024;
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
  /** Lane 17: the `net` entries the person saw in the dialog (absent on older grants, which
   * then cover no network: a plugin with `net` is enabled again once). */
  net?: string[];
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

/** Whether a grant still covers what the manifest asks for: its permissions and (lane 17)
 * every `net` entry. */
export function grantCovers(g: Grant | undefined, permissions: Permission[], net: string[] = []): boolean {
  if (!g || !Array.isArray(g.permissions) || !permissions.every((p) => g.permissions.includes(p))) return false;
  const granted = Array.isArray(g.net) ? g.net : [];
  return net.every((n) => granted.includes(n));
}

/**
 * Lane 17: a grant cut down to what the manifest still asks for. A grant used to be changed
 * only by Enable, so a source that keeps its grant across changes (a bundled sample, an
 * unpacked folder) could drop a permission and later ask for it again with no new consent.
 * Returns `g` itself when nothing is cut.
 */
export function trimGrant(g: Grant, permissions: Permission[], net: string[] = []): Grant {
  const keep = Array.isArray(g.permissions) ? g.permissions.filter((p) => permissions.includes(p)) : [];
  const keepNet = Array.isArray(g.net) ? g.net.filter((n) => net.includes(n)) : undefined;
  const same = keep.length === (g.permissions?.length ?? 0) && (keepNet === undefined || keepNet.length === g.net!.length);
  if (same) return g;
  return { ...g, permissions: keep, ...(keepNet !== undefined ? { net: keepNet } : {}) };
}

const checkKey = (key: unknown): string => {
  if (typeof key !== "string" || key.length < 1 || key.length > 200) throw new TypeError("a storage key is a string of 1 to 200 characters");
  return key;
};

/** Lane 15: a key read as the store's own entry only, so "constructor", "toString" or
 * "__proto__" are keys like any other (not Object.prototype's members). */
function own(rec: Record<string, unknown> | undefined, key: string): unknown {
  return rec && Object.prototype.hasOwnProperty.call(rec, key) ? rec[key] : undefined;
}

/** Set an own entry, even for "__proto__" (a plain assignment would change the prototype). */
function put(rec: Record<string, unknown>, key: string, value: unknown): void {
  Object.defineProperty(rec, key, { value, enumerable: true, writable: true, configurable: true });
}

/**
 * The length JSON.stringify would give `v`, measured without building the string, stopping
 * as soon as it passes `limit` (lane 16: a 32 MiB byte array used to be stringified to about
 * 400 MB before the quota was compared). Only JSON values are storable: a byte array, a
 * function or a non-finite number is refused.
 */
export function jsonSize(v: unknown, limit: number): number {
  let n = 0;
  const over = () => n > limit;
  const text = (s: string) => {
    n += 2;
    for (let i = 0; i < s.length; i++) {
      const c = s.charCodeAt(i);
      if (c === 0x22 || c === 0x5c) n += 2;
      else if (c < 0x20) n += c === 8 || c === 9 || c === 10 || c === 12 || c === 13 ? 2 : 6;
      else if (c >= 0xd800 && c <= 0xdfff) {
        const hi = c <= 0xdbff;
        const next = s.charCodeAt(i + 1);
        if (hi && next >= 0xdc00 && next <= 0xdfff) {
          n += 2;
          i++;
        } else n += 6;
      } else n += 1;
      if ((i & 1023) === 0 && over()) return;
    }
  };
  const walk = (x: unknown, depth: number): void => {
    if (over()) return;
    if (depth > 64) throw new TypeError("a stored value is nested more than 64 deep");
    if (x === null || x === undefined) n += 4;
    else if (x === true) n += 4;
    else if (x === false) n += 5;
    else if (typeof x === "number") {
      if (!Number.isFinite(x)) throw new TypeError("a stored number must be finite");
      n += String(x).length;
    } else if (typeof x === "string") text(x);
    else if (x instanceof Uint8Array) throw new TypeError("storage keeps JSON values (text, numbers, true/false, null, lists, objects), not byte arrays");
    else if (Array.isArray(x)) {
      n += 2 + Math.max(0, x.length - 1);
      for (let i = 0; i < x.length && !over(); i++) walk(x[i], depth + 1);
    } else if (typeof x === "object") {
      n += 2;
      let first = true;
      for (const k of Object.keys(x)) {
        const y = (x as Record<string, unknown>)[k];
        if (y === undefined) continue;
        if (!first) n += 1;
        first = false;
        text(k);
        n += 1;
        walk(y, depth + 1);
        if (over()) return;
      }
    } else throw new TypeError(`storage keeps JSON values, not a ${typeof x}`);
  };
  walk(v, 0);
  return n;
}

function sizeOf(v: unknown, limit = Number.MAX_SAFE_INTEGER): number {
  return jsonSize(v ?? null, limit);
}

/** What every plugin's app store takes in localStorage, except `exceptKey`'s. */
function appStorageUsed(exceptKey: string): number {
  let n = 0;
  try {
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (!k || !k.startsWith(KV_PREFIX) || k === exceptKey) continue;
      n += localStorage.getItem(k)?.length ?? 0;
    }
  } catch {
    /* nothing kept anyway */
  }
  return n;
}

/** Parsed app stores, so a get or set does not parse and stringify the whole store each time. */
const appCache = new Map<string, Record<string, unknown>>();
if (typeof window !== "undefined")
  try {
    // Another tab changed a store: read it again next time.
    window.addEventListener("storage", (e) => (e.key ? appCache.delete(e.key) : appCache.clear()));
  } catch {
    /* no events here */
  }

/** A plugin's app-scoped store (localStorage). */
export class AppStore {
  private key: string;
  constructor(pluginId: string) {
    this.key = `${KV_PREFIX}${pluginId}`;
  }
  private all(): Record<string, unknown> {
    const hit = appCache.get(this.key);
    if (hit) return hit;
    const v = readJson<unknown>(this.key, {});
    const rec: Record<string, unknown> = {};
    // Own entries only, "__proto__" included as a plain key (lane 16).
    if (v && typeof v === "object" && !Array.isArray(v)) for (const k of Object.keys(v)) put(rec, k, (v as Record<string, unknown>)[k]);
    appCache.set(this.key, rec);
    return rec;
  }
  get(key: unknown): unknown {
    return own(this.all(), checkKey(key)) ?? null;
  }
  set(key: unknown, value: unknown): void {
    const k = checkKey(key);
    // The value alone first, with an early stop: an oversized value is refused before anything
    // is built for it.
    const size = sizeOf(value, APP_QUOTA);
    if (size > APP_QUOTA) throw new RangeError(`the plugin's store would be over ${APP_QUOTA / 1024} KB`);
    const next: Record<string, unknown> = {};
    for (const [kk, vv] of Object.entries(this.all())) put(next, kk, vv);
    put(next, k, value);
    const json = JSON.stringify(next);
    if (json.length > APP_QUOTA) throw new RangeError(`the plugin's store would be over ${APP_QUOTA / 1024} KB`);
    if (appStorageUsed(this.key) + json.length > APP_TOTAL) throw new RangeError(`the plugins' stores together would be over ${APP_TOTAL / 1024} KB in this browser: clear another plugin's data (uninstall it) first`);
    try {
      localStorage.setItem(this.key, json);
    } catch {
      throw new Error("this browser is not keeping data for the app (private window or storage blocked)");
    }
    appCache.set(this.key, next);
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
    appCache.delete(this.key);
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
    return own(own(this.data, plugin) as Record<string, unknown> | undefined, checkKey(key)) ?? null;
  }
  set(plugin: string, key: unknown, value: unknown): void {
    const k = checkKey(key);
    if (sizeOf(value, DOC_QUOTA) > DOC_QUOTA) throw new RangeError(`the plugin's document store would be over ${DOC_QUOTA / 1024} KB`);
    const next: Record<string, unknown> = {};
    for (const [kk, vv] of Object.entries((own(this.data, plugin) as Record<string, unknown> | undefined) ?? {})) put(next, kk, vv);
    put(next, k, value);
    const size = sizeOf(next, DOC_QUOTA);
    if (size > DOC_QUOTA) throw new RangeError(`the plugin's document store would be over ${DOC_QUOTA / 1024} KB`);
    let others = 0;
    for (const [p, rec] of Object.entries(this.data)) if (p !== plugin) others += sizeOf(rec, DOC_TOTAL);
    if (others + size > DOC_TOTAL) throw new RangeError(`the plugins' document stores together would be over ${DOC_TOTAL / 1024} KB`);
    put(this.data, plugin, next);
  }
  delete(plugin: string, key: unknown): void {
    const rec = own(this.data, plugin) as Record<string, unknown> | undefined;
    if (rec) delete rec[checkKey(key)];
  }
  keys(plugin: string): string[] {
    return Object.keys((own(this.data, plugin) as Record<string, unknown> | undefined) ?? {});
  }
  /** A new document: nothing stored. */
  clear(): void {
    this.data = {};
  }
  snapshot(): Record<string, Record<string, unknown>> {
    return JSON.parse(JSON.stringify(this.data)) as Record<string, Record<string, unknown>>;
  }
  /**
   * Take the stores a document brought (plugin-data.json's `plugins`). Every key is set as an
   * own entry (lane 16: a "__proto__" key from a document used to re-parent the store), a
   * store over the per-plugin quota is left out, and so is everything past the total.
   */
  load(data: unknown): void {
    this.data = {};
    if (!data || typeof data !== "object" || Array.isArray(data)) return;
    let total = 0;
    for (const k of Object.keys(data)) {
      const v = (data as Record<string, unknown>)[k];
      if (!v || typeof v !== "object" || Array.isArray(v) || k === "__proto__") continue;
      let size: number;
      try {
        size = sizeOf(v, DOC_QUOTA);
      } catch {
        continue;
      }
      if (size > DOC_QUOTA || total + size > DOC_TOTAL) continue;
      total += size;
      const rec: Record<string, unknown> = {};
      for (const kk of Object.keys(v)) put(rec, kk, (v as Record<string, unknown>)[kk]);
      put(this.data, k, rec);
    }
  }
  empty(): boolean {
    return Object.values(this.data).every((v) => Object.keys(v).length === 0);
  }

  /** plugin-data.json as the kernel writes it into a saved .shz (null when nothing is stored). */
  json(): string | null {
    return this.empty() ? null : JSON.stringify({ version: 1, plugins: this.data });
  }

  /**
   * Take a document's plugin-data.json (as the kernel read it from the .shz, capped there):
   * `{"version": 1, "plugins": {...}}`. Text that is not that, or too long, is an empty
   * store (lane 16: the app used to unzip the user's file itself to find it).
   */
  loadJson(text: string | null): void {
    this.clear();
    if (!text || text.length > 4 * DOC_TOTAL) return;
    try {
      this.load((JSON.parse(text) as { plugins?: unknown }).plugins);
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
