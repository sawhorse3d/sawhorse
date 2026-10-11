// Dev autosave: the document's op log, kept in this browser's IndexedDB after every edit,
// so a reload (or a crashed tab) does not lose work before real files (.shz, lane 6) exist.
// On the next load the app offers "Restore unsaved work?" in the status line; restoring
// replays the log in the kernel (undo history included), discarding forgets it.
//
// Per-browser convenience only: storage can be blocked, cleared or full, so every access
// is wrapped and failures are silent (the app works the same without it). `?autosave=0`
// turns it off for a page load.
import type { KernelOp } from "../kernel/types";

export interface Autosave {
  /** The op log (the kernel's history format; undo and redo entries included). */
  log: KernelOp[];
  /** Milliseconds since the epoch. */
  savedAt: number;
  /** Solids in the saved document (for the prompt). */
  solids: number;
}

const DB = "sawhorse";
const STORE = "autosave";
const KEY = "current";

/** Autosave is on unless the URL says `autosave=0`. */
export function autosaveEnabled(search = typeof location !== "undefined" ? location.search : ""): boolean {
  return new URLSearchParams(search).get("autosave") !== "0";
}

/** A log worth offering back: more than the ground sketch that every document starts with. */
export function worthRestoring(a: Autosave | null): a is Autosave {
  return !!a && Array.isArray(a.log) && a.log.some((op) => op.op !== "new_sketch" && op.op !== "undo" && op.op !== "redo");
}

function open(): Promise<IDBDatabase | null> {
  return new Promise((resolve) => {
    try {
      if (typeof indexedDB === "undefined") return resolve(null);
      const req = indexedDB.open(DB, 1);
      req.onupgradeneeded = () => {
        if (!req.result.objectStoreNames.contains(STORE)) req.result.createObjectStore(STORE);
      };
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => resolve(null);
      req.onblocked = () => resolve(null);
    } catch {
      resolve(null);
    }
  });
}

async function withStore<T>(mode: IDBTransactionMode, f: (s: IDBObjectStore) => IDBRequest<T> | null): Promise<T | null> {
  const db = await open();
  if (!db) return null;
  return new Promise((resolve) => {
    try {
      const tx = db.transaction(STORE, mode);
      const req = f(tx.objectStore(STORE));
      let value: T | null = null;
      if (req) req.onsuccess = () => (value = req.result ?? null);
      tx.oncomplete = () => {
        db.close();
        resolve(value);
      };
      tx.onerror = tx.onabort = () => {
        db.close();
        resolve(null);
      };
    } catch {
      db.close();
      resolve(null);
    }
  });
}

export async function loadAutosave(): Promise<Autosave | null> {
  const v = await withStore<Autosave>("readonly", (s) => s.get(KEY));
  return v && Array.isArray(v.log) ? v : null;
}

export async function saveAutosave(a: Autosave): Promise<void> {
  await withStore("readwrite", (s) => s.put(a, KEY));
}

export async function clearAutosave(): Promise<void> {
  await withStore("readwrite", (s) => s.delete(KEY));
}

/** "2 solids, saved 3 minutes ago" for the prompt. */
export function describeAutosave(a: Autosave, now = Date.now()): string {
  const mins = Math.max(0, Math.round((now - a.savedAt) / 60000));
  const when = mins < 1 ? "just now" : mins === 1 ? "1 minute ago" : mins < 120 ? `${mins} minutes ago` : `${Math.round(mins / 60)} hours ago`;
  return `${a.solids} solid${a.solids === 1 ? "" : "s"}, ${a.log.length} steps, saved ${when}`;
}
