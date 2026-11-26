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
// or huge autosave cannot block the app from starting.
//
// Lane 64 (#94): what is found at start is set aside ("offered") before this session
// autosaves, so the first edit never overwrites it, and every piece of work set aside stays
// until the person answers about that piece. Before lane 64 there was one offer: an offer
// left unanswered stayed the offer at every later start, and the newer work of the session
// in between was kept in `current` and never offered; Restore of the old offer then wrote
// the old work over it. Now the store holds a list of offers, one pair of keys each: the log
// under its id and the record under `<id>-meta`. The first id is `offered` (the pair older
// builds used, so what they set aside is read as one offer as it is) and the next are
// `offered-2`, `offered-3`, ...; `current` and `current-meta` are this session's, as before.
// At start this session's predecessor (`current`) joins the list, an exact copy of another
// pair's log is kept once, the list is offered newest first, and at most MAX_OFFERS are kept:
// past that the oldest is deleted and the start says so. Restore moves the open model out of
// `current` into a pair of its own when it holds unsaved work, then moves its offer into
// `current` (the restored work is this session's work now); Discard deletes it.
import type { KernelOp } from "../kernel/types";

/** Largest op log autosaved (characters of JSON): past it, autosave stops for that model. */
export const MAX_AUTOSAVE_BYTES = 64 * 1024 * 1024;

/**
 * Lane 64: the most pieces of unsaved work kept set aside, each at most MAX_AUTOSAVE_BYTES.
 * A start that would keep one more deletes the oldest and says so.
 */
export const MAX_OFFERS = 5;

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
  /**
   * #149: AUTOSAVE_RULES when the log was written by a build that keeps what was drawn in one
   * copy of a component when the component loses the group it was in. Absent: an older build
   * wrote it, and Restore replays it guarded (refused where the two builds differ).
   */
  rules?: number;
}

/** #149: the mark this build writes on every autosave record (AutosaveMeta.rules). */
export const AUTOSAVE_RULES = 1;

/** Whether a record was written by a build before the mark (its log is replayed guarded). */
export function unmarked(a: AutosaveMeta): boolean {
  return typeof a.rules !== "number";
}

/** An autosave found at start: its record, and whether its log can be restored. */
export interface Autosave extends AutosaveMeta {
  /** "ok", or why it cannot be restored (it is still offered for discarding). */
  state: "ok" | "too_large" | "unreadable";
}

/** Lane 64: one piece of unsaved work set aside, by the key its log is kept under. */
export interface Offer extends Autosave {
  id: string;
}

/** What a start found: the offers, newest first, and any deleted to stay within MAX_OFFERS. */
export interface Found {
  offers: Offer[];
  dropped: Offer[];
}

const DB = "sawhorse";
const STORE = "autosave";
const KEY = "current";
const META = "current-meta";
/** The first offer's id, and the only one before lane 64. */
const OFFERED = "offered";
const OFFER_ID = /^offered(?:-([0-9]+))?$/;
const OFFER_META = /^(offered(?:-[0-9]+)?)-meta$/;

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

/**
 * One transaction on the store. `f` issues its requests and may `set` the value the promise
 * resolves to; it resolves when the transaction completes, to null when it fails.
 */
async function transact<T>(mode: IDBTransactionMode, f: (s: IDBObjectStore, set: (v: T) => void) => void): Promise<T | null> {
  const db = await open();
  if (!db) return null;
  return new Promise((resolve) => {
    try {
      const tx = db.transaction(STORE, mode);
      let value: T | null = null;
      tx.oncomplete = () => {
        db.close();
        resolve(value);
      };
      tx.onerror = tx.onabort = () => {
        db.close();
        resolve(null);
      };
      f(tx.objectStore(STORE), (v) => (value = v));
    } catch {
      db.close();
      resolve(null);
    }
  });
}

/** One request's result (null when it is absent or the request fails). */
function withStore<T>(mode: IDBTransactionMode, f: (s: IDBObjectStore) => IDBRequest<T> | null | void): Promise<T | null> {
  return transact<T>(mode, (s, set) => {
    const req = f(s);
    if (req) req.onsuccess = () => set(req.result ?? (null as T));
  });
}

const isMeta = (v: unknown): v is AutosaveMeta => {
  const m = v as AutosaveMeta;
  return !!m && typeof m === "object" && Number.isFinite(m.savedAt) && Number.isFinite(m.solids) && Number.isFinite(m.steps) && Number.isFinite(m.bytes) && typeof m.worth === "boolean";
};

/** The offer a record describes; one it cannot make sense of is "unreadable" (Discard removes it). */
function offerOf(id: string, meta: unknown): Offer {
  if (!isMeta(meta)) return { id, savedAt: 0, solids: 0, steps: 0, bytes: 0, worth: true, state: "unreadable" };
  return { id, ...meta, state: meta.bytes > MAX_AUTOSAVE_BYTES ? "too_large" : "ok" };
}

const offerNumber = (id: string) => {
  const m = OFFER_ID.exec(id);
  return m ? (m[1] ? Number(m[1]) : 1) : 0;
};

/** Newest first; a record with no time (unreadable, or from before lane 16) last. */
export function newestFirst(a: Offer, b: Offer): number {
  const ta = a.state === "unreadable" ? -1 : a.savedAt;
  const tb = b.state === "unreadable" ? -1 : b.savedAt;
  return tb - ta || offerNumber(b.id) - offerNumber(a.id);
}

/** An id no offer uses: `offered` when it is free, otherwise one past the highest number. */
export function freeOfferId(keys: readonly unknown[]): string {
  const used = new Set(keys.filter((k): k is string => typeof k === "string"));
  if (!used.has(OFFERED) && !used.has(`${OFFERED}-meta`)) return OFFERED;
  let n = 1;
  for (const k of used) {
    const m = OFFER_ID.exec(k.replace(/-meta$/, ""));
    if (m) n = Math.max(n, m[1] ? Number(m[1]) : 1);
  }
  return `${OFFERED}-${n + 1}`;
}

const isOfferId = (id: string) => OFFER_ID.test(id);

/**
 * At start: set this browser's unsaved work aside, out of the way of this session's
 * autosaves, and describe every piece of it, newest first. The last session's work
 * (`current`) joins what earlier starts set aside and nobody answered. Logs move inside the
 * database; one is read into this page only to tell an exact copy of another (same size and
 * steps). Never throws, and returns nothing to offer
 * when storage cannot be used.
 */
export async function takeAutosaves(): Promise<Found> {
  const none: Found = { offers: [], dropped: [] };
  try {
    const keys = (await withStore<IDBValidKey[]>("readonly", (s) => s.getAllKeys())) ?? [];
    if (!keys.length) return none;
    const has = (k: string) => keys.includes(k);
    const cur: unknown = has(META) ? await withStore("readonly", (s) => s.get(META)) : null;
    // An autosave from before lane 16 has a log array and no record beside it.
    const legacy = cur === null && has(KEY) ? await legacyMeta() : null;
    const last = cur ?? legacy;
    const found = await transact<Offer[]>("readwrite", (s, set) => {
      const offers: Offer[] = [];
      const ids = new Set<string>();
      for (const k of keys) {
        if (typeof k !== "string") continue;
        const meta = OFFER_META.exec(k);
        if (meta) ids.add(meta[1]);
        else if (isOfferId(k)) ids.add(k);
      }
      // The last session's work, as a new offer (a log that holds no work is not kept).
      if (last !== null) {
        if (isMeta(last) && !last.worth) {
          s.delete(KEY);
          s.delete(META);
        } else {
          const id = freeOfferId(keys);
          const get = s.get(KEY);
          get.onsuccess = () => {
            if (get.result !== undefined) s.put(get.result, id);
            s.put(last, `${id}-meta`);
            s.delete(KEY);
            s.delete(META);
          };
          offers.push(offerOf(id, last));
        }
      }
      // What earlier starts set aside: each read by its record.
      for (const id of ids) {
        const get = s.get(`${id}-meta`);
        get.onsuccess = () => {
          const meta: unknown = get.result;
          if (isMeta(meta) && !meta.worth) {
            s.delete(id);
            s.delete(`${id}-meta`);
            return;
          }
          offers.push(offerOf(id, meta));
        };
      }
      set(offers);
    });
    if (!found) return none;
    found.sort(newestFirst);
    // The same log in two pairs (a copy left by an older build of this lane, or two windows
    // that set aside the same state) is kept once, the newest, and counts once toward
    // MAX_OFFERS. Only a log equal to another character for character goes, and silently: the
    // work is still there. Logs are read only where two records agree in size and steps.
    const copies = await exactCopies(found);
    if (copies.size) {
      await transact("readwrite", (s) => {
        for (const id of copies) {
          s.delete(id);
          s.delete(`${id}-meta`);
        }
      });
      for (let i = found.length - 1; i >= 0; i--) if (copies.has(found[i].id)) found.splice(i, 1);
    }
    const dropped = found.splice(MAX_OFFERS);
    if (dropped.length) {
      await transact("readwrite", (s) => {
        for (const d of dropped) {
          s.delete(d.id);
          s.delete(`${d.id}-meta`);
        }
      });
    }
    return { offers: found, dropped };
  } catch {
    return none;
  }
}

/** The ids of offers whose log is an exact copy of an offer's before them in `sorted`. */
async function exactCopies(sorted: readonly Offer[]): Promise<Set<string>> {
  const copies = new Set<string>();
  const groups = new Map<string, Offer[]>();
  for (const o of sorted) {
    if (o.state !== "ok") continue;
    const k = `${o.bytes}/${o.steps}`;
    groups.set(k, [...(groups.get(k) ?? []), o]);
  }
  for (const group of groups.values()) {
    if (group.length < 2) continue;
    const kept: string[] = [];
    for (const o of group) {
      const log = await loadOffered(o.id);
      if (log === null) continue;
      if (kept.includes(log)) copies.add(o.id);
      else kept.push(log);
    }
  }
  return copies;
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

/** An offer's log as JSON text, when it can be restored (null otherwise). */
export async function loadOffered(id: string): Promise<string | null> {
  if (!isOfferId(id)) return null;
  const meta: unknown = await withStore("readonly", (s) => s.get(`${id}-meta`));
  if (!isMeta(meta) || meta.bytes > MAX_AUTOSAVE_BYTES) return null;
  const v: unknown = await withStore("readonly", (s) => s.get(id));
  return typeof v === "string" && v.length <= MAX_AUTOSAVE_BYTES ? v : null;
}

/** Forget one offer (Discard). */
export async function clearOffered(id: string): Promise<void> {
  if (!isOfferId(id)) return;
  await transact("readwrite", (s) => {
    s.delete(id);
    s.delete(`${id}-meta`);
  });
}

/**
 * Restore: the offer becomes this session's autosave (`current`), in one step, so the
 * restored work is never only in memory and never kept twice. (Also how a model set aside
 * by a Restore that then failed goes back to being this session's autosave.) What `current` held is
 * replaced: the caller has set aside any work it holds (setAside) first. False when storage
 * failed.
 */
export async function adoptOffered(id: string): Promise<boolean> {
  if (!isOfferId(id)) return false;
  const done = await transact<boolean>("readwrite", (s, set) => {
    const meta = s.get(`${id}-meta`);
    meta.onsuccess = () => {
      const log = s.get(id);
      log.onsuccess = () => {
        if (log.result === undefined || meta.result === undefined) return;
        s.put(log.result, KEY);
        s.put(meta.result, META);
        s.delete(id);
        s.delete(`${id}-meta`);
        set(true);
      };
    };
  });
  return done === true;
}

/**
 * Lane 64: set this session's model aside as one more offer, before Restore puts other work in
 * its place. A move, not a copy: this session's autosave (`current`, an older state of the
 * same model or nothing) goes in the same step, so a reload or a crash before Restore ends
 * finds the model kept once. Null when it is not kept (over MAX_AUTOSAVE_BYTES, or storage
 * failed), and then `current` is left as it is.
 */
export async function setAside(json: string, meta: Omit<AutosaveMeta, "bytes">): Promise<Offer | null> {
  if (json.length > MAX_AUTOSAVE_BYTES) return null;
  const record: AutosaveMeta = { ...meta, bytes: json.length, rules: AUTOSAVE_RULES };
  return transact<Offer>("readwrite", (s, set) => {
    const keys = s.getAllKeys();
    keys.onsuccess = () => {
      const id = freeOfferId(keys.result);
      s.put(json, id);
      s.put(record, `${id}-meta`);
      s.delete(KEY);
      s.delete(META);
      set({ id, ...record, state: "ok" });
    };
  });
}

/** Keep this session's log. A log over MAX_AUTOSAVE_BYTES is not kept (false). */
export async function saveAutosave(json: string, meta: Omit<AutosaveMeta, "bytes">): Promise<boolean> {
  if (json.length > MAX_AUTOSAVE_BYTES) {
    await clearAutosave();
    return false;
  }
  await transact("readwrite", (s) => {
    s.put(json, KEY);
    s.put({ ...meta, bytes: json.length, rules: AUTOSAVE_RULES }, META);
  });
  return true;
}

/** Forget this session's autosave (the model was saved, or replaced). Offers are not touched. */
export async function clearAutosave(): Promise<void> {
  await transact("readwrite", (s) => {
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

/** Lane 64: said after the offer when more work is set aside behind it. */
export function olderNote(n: number): string {
  return n === 1 ? "1 older set of unsaved work is set aside too." : `${n} older sets of unsaved work are set aside too.`;
}

/** Lane 64: said at start when the oldest work was deleted to keep MAX_OFFERS. */
export function droppedNote(dropped: readonly Autosave[], now = Date.now()): string {
  if (!dropped.length) return "";
  const what = dropped.map((d) => describeAutosave(d, now)).join("; ");
  return dropped.length === 1
    ? `At most ${MAX_OFFERS} sets of unsaved work are kept: the oldest (${what}) was deleted.`
    : `At most ${MAX_OFFERS} sets of unsaved work are kept: the ${dropped.length} oldest (${what}) were deleted.`;
}

/** Lane 64: the status line's reminder while work is set aside and its offer is not on screen. */
export function setAsideNote(n: number): string {
  return `Unsaved work set aside${n > 1 ? ` (${n})` : ""}. Click to restore or discard.`;
}
