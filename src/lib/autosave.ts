// Dev autosave: the document's op log, kept in this browser's IndexedDB after every edit,
// so a reload (or a crashed tab) does not lose work before real files (.shz, lane 6) exist.
// On the next load the app offers "Restore unsaved work?" in the status line; restoring
// replays the log in the kernel (undo history included), discarding forgets it.
//
// Per-browser convenience only: storage can be blocked, cleared or full, so every access
// is wrapped and failures are silent (the app works the same without it). `?autosave=0`
// turns it off for a page load.
//
// Lane 16: the log is kept as one JSON text (what the kernel replays; the app never parses
// it), with a small record beside it (its size, steps, solids, whether it is worth
// offering) that is read first: a log over MAX_AUTOSAVE_BYTES is never written, and one
// found over it (or unreadable) is never loaded, only offered for discarding, so a broken
// or huge autosave cannot block the app from starting. What is found at start is set aside
// ("offered") before this session autosaves, so the first edit never overwrites an
// unanswered offer; Restore reads it from there, Discard deletes it.
import type { KernelOp } from "../kernel/types";

/** Largest op log autosaved (characters of JSON): past it, autosave stops for that model. */
export const MAX_AUTOSAVE_BYTES = 64 * 1024 * 1024;

/** What is kept about an autosave besides the log itself (small: read before the log). */
export interface AutosaveMeta {
  /** Milliseconds since the epoch. */
  savedAt: number;
  /** Solids in the saved document (for the prompt). */
  solids: number;
  /** Entries in the log. */
  steps: number;
  /** Characters of the log's JSON. */
  bytes: number;
  /** More than the ground sketch every document starts with. */
  worth: boolean;
}

/** An autosave found at start: its record, and whether its log can be restored. */
export interface Autosave extends AutosaveMeta {
  /** "ok", or why it cannot be restored (it is still offered for discarding). */
  state: "ok" | "too_large" | "unreadable";
}

const DB = "sawhorse";
const STORE = "autosave";
const KEY = "current";
const META = "current-meta";
const OFFERED = "offered";
const OFFERED_META = "offered-meta";

/** Autosave is on unless the URL says `autosave=0`. */
export function autosaveEnabled(search = typeof location !== "undefined" ? location.search : ""): boolean {
  return new URLSearchParams(search).get("autosave") !== "0";
}

/** A log worth offering back: more than the ground sketch that every document starts with. */
export function worthRestoring(a: Autosave | AutosaveMeta | null): a is Autosave {
  return !!a && typeof a === "object" && a.worth === true;
}

/** Whether an op log (JSON texts, one per op) holds more than the ground sketch and history moves. */
export function logIsWorth(ops: readonly string[]): boolean {
  return ops.some((o) => !/^\{"op":"(new_sketch|undo|redo)"/.test(o));
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

async function withStore<T>(mode: IDBTransactionMode, f: (s: IDBObjectStore) => IDBRequest<T> | null | void): Promise<T | null> {
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

const isMeta = (v: unknown): v is AutosaveMeta => {
  const m = v as AutosaveMeta;
  return !!m && typeof m === "object" && Number.isFinite(m.savedAt) && Number.isFinite(m.solids) && Number.isFinite(m.steps) && Number.isFinite(m.bytes) && typeof m.worth === "boolean";
};

/**
 * At start: set what this browser kept aside as the offer (moving it out of the way of this
 * session's autosaves) and describe it. Never throws; a record it cannot make sense of is
 * offered as "unreadable" (Discard removes it).
 */
export async function takeAutosave(): Promise<Autosave | null> {
  try {
    // An offer still unanswered from an earlier start stays the offer.
    let meta: unknown = await withStore("readonly", (s) => s.get(OFFERED_META));
    if (meta === null) {
      const cur: unknown = await withStore("readonly", (s) => s.get(META));
      // An autosave from before lane 16 has a log array and no record beside it.
      const legacy = cur === null ? await legacyMeta() : null;
      if (cur === null && legacy === null) return null;
      await withStore("readwrite", (s) => {
        // Moved inside the database, never through this page's memory.
        const get = s.get(KEY);
        get.onsuccess = () => {
          if (get.result !== undefined) s.put(get.result, OFFERED);
          s.put(cur ?? legacy, OFFERED_META);
          s.delete(KEY);
          s.delete(META);
        };
      });
      meta = cur ?? legacy;
    }
    if (!isMeta(meta)) return { savedAt: 0, solids: 0, steps: 0, bytes: 0, worth: true, state: "unreadable" };
    return { ...meta, state: meta.bytes > MAX_AUTOSAVE_BYTES ? "too_large" : "ok" };
  } catch {
    return null;
  }
}

/** A pre-lane-16 record (a log array): its record, worked out once. */
async function legacyMeta(): Promise<AutosaveMeta | null> {
  const v = await withStore<{ log?: unknown; savedAt?: unknown; solids?: unknown }>("readonly", (s) => s.get(KEY));
  if (!v || typeof v !== "object") return null;
  const log = Array.isArray(v.log) ? (v.log as KernelOp[]) : null;
  if (!log) return { savedAt: 0, solids: 0, steps: 0, bytes: MAX_AUTOSAVE_BYTES + 1, worth: true };
  // Too many entries to be worth turning into text here: offered for discarding only.
  if (log.length > 200_000) return { savedAt: 0, solids: 0, steps: log.length, bytes: MAX_AUTOSAVE_BYTES + 1, worth: true };
  const text = JSON.stringify(log);
  await withStore("readwrite", (s) => s.put(text, KEY));
  return {
    savedAt: typeof v.savedAt === "number" ? v.savedAt : 0,
    solids: typeof v.solids === "number" ? v.solids : 0,
    steps: log.length,
    bytes: text.length,
    worth: log.some((op) => !!op && typeof op === "object" && op.op !== "new_sketch" && op.op !== "undo" && op.op !== "redo"),
  };
}

/** The offered log's JSON text, when it can be restored (null otherwise). */
export async function loadOffered(): Promise<string | null> {
  const meta: unknown = await withStore("readonly", (s) => s.get(OFFERED_META));
  if (!isMeta(meta) || meta.bytes > MAX_AUTOSAVE_BYTES) return null;
  const v: unknown = await withStore("readonly", (s) => s.get(OFFERED));
  return typeof v === "string" && v.length <= MAX_AUTOSAVE_BYTES ? v : null;
}

/** Forget the offer (Discard, or after it was restored). */
export async function clearOffered(): Promise<void> {
  await withStore("readwrite", (s) => {
    s.delete(OFFERED);
    s.delete(OFFERED_META);
  });
}

/** Keep this session's log. A log over MAX_AUTOSAVE_BYTES is not kept (false). */
export async function saveAutosave(json: string, meta: Omit<AutosaveMeta, "bytes">): Promise<boolean> {
  if (json.length > MAX_AUTOSAVE_BYTES) {
    await clearAutosave();
    return false;
  }
  await withStore("readwrite", (s) => {
    s.put(json, KEY);
    s.put({ ...meta, bytes: json.length }, META);
  });
  return true;
}

/** Forget this session's autosave (the model was saved, or replaced). */
export async function clearAutosave(): Promise<void> {
  await withStore("readwrite", (s) => {
    s.delete(KEY);
    s.delete(META);
  });
}

/** "2 solids, saved 3 minutes ago" for the prompt. */
export function describeAutosave(a: Autosave, now = Date.now()): string {
  if (a.state === "unreadable") return "the record kept in this browser cannot be read";
  const mins = Math.max(0, Math.round((now - a.savedAt) / 60000));
  const when = mins < 1 ? "just now" : mins === 1 ? "1 minute ago" : mins < 120 ? `${mins} minutes ago` : `${Math.round(mins / 60)} hours ago`;
  const size = a.state === "too_large" ? ` (${Math.round(a.bytes / 1024 / 1024)} MB of history: too large to restore here)` : "";
  return `${a.solids} solid${a.solids === 1 ? "" : "s"}, ${a.steps} steps, saved ${when}${size}`;
}
