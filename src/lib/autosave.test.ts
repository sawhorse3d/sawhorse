import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  adoptOffered,
  autosaveEnabled,
  clearAutosave,
  clearOffered,
  describeAutosave,
  droppedNote,
  freeOfferId,
  loadOffered,
  logIsWorth,
  MAX_AUTOSAVE_BYTES,
  MAX_OFFERS,
  newestFirst,
  olderNote,
  saveAutosave,
  setAside,
  setAsideNote,
  takeAutosaves,
  worthRestoring,
  type Autosave,
  type Offer,
} from "./autosave";

const meta = (over: Partial<Autosave> = {}): Autosave => ({ savedAt: 0, solids: 2, steps: 2, bytes: 100, worth: true, state: "ok", ...over });

describe("dev autosave", () => {
  it("offers back only real work", () => {
    expect(worthRestoring(null)).toBe(false);
    expect(worthRestoring(meta({ worth: false }))).toBe(false);
    expect(worthRestoring(meta())).toBe(true);
    // Lane 16: worked out from the log's texts, never by parsing the log.
    expect(logIsWorth(['{"op":"new_sketch","origin":[0,0,0],"normal":[0,0,1]}', '{"op":"undo"}'])).toBe(false);
    expect(logIsWorth(['{"op":"new_sketch","origin":[0,0,0],"normal":[0,0,1]}', '{"op":"add_rectangle","sketch":1,"x":0,"y":0,"w":1,"h":1}'])).toBe(true);
  });

  it("is on unless the URL turns it off", () => {
    expect(autosaveEnabled("")).toBe(true);
    expect(autosaveEnabled("?autosave=0")).toBe(false);
    expect(autosaveEnabled("?theme=dark&autosave=1")).toBe(true);
  });

  it("says what would come back, and why it cannot", () => {
    expect(describeAutosave(meta(), 0)).toBe("2 solids, 2 steps, saved just now");
    expect(describeAutosave(meta({ solids: 1 }), 5 * 60000)).toBe("1 solid, 2 steps, saved 5 minutes ago");
    expect(describeAutosave(meta({ state: "too_large", bytes: 100 * 1024 * 1024 }), 0)).toContain("100 MB of history: too large to restore here");
    expect(describeAutosave(meta({ state: "unreadable" }), 0)).toBe("the record kept in this browser cannot be read");
  });

  it("never keeps a log over the limit, and fails quietly without IndexedDB (jsdom)", async () => {
    expect(await saveAutosave("x".repeat(MAX_AUTOSAVE_BYTES + 1), { savedAt: 1, solids: 0, steps: 1, worth: true })).toBe(false);
    await saveAutosave("[]", { savedAt: 1, solids: 0, steps: 0, worth: false });
    expect(await takeAutosaves()).toEqual({ offers: [], dropped: [] });
    expect(await loadOffered("offered")).toBeNull();
    expect(await setAside("[]", { savedAt: 1, solids: 0, steps: 0, worth: true })).toBeNull();
    expect(await adoptOffered("offered")).toBe(false);
    await clearOffered("offered");
  });
});

// ---- lane 64 (#94): every piece of unsaved work set aside, offered newest first -------------
//
// jsdom has no IndexedDB and the repo has no implementation of it for tests, so these run on
// the stand-in below: one database of named stores, transactions whose requests run in order
// after the code that made them returns, `oncomplete` once no request is left (a request made
// in another's `onsuccess` belongs to the same transaction), values copied in and out.

type Value = unknown;
interface Req<T = Value> {
  result: T;
  onsuccess: (() => void) | null;
  onerror: (() => void) | null;
  onupgradeneeded?: (() => void) | null;
  onblocked?: (() => void) | null;
}

class FakeIDB {
  stores = new Map<string, Map<string, Value>>();
  /** Every open fails (blocked, cleared or broken storage). */
  failing = false;

  open(): Req<FakeDB> {
    const req: Req<FakeDB> = { result: undefined as unknown as FakeDB, onsuccess: null, onerror: null, onupgradeneeded: null, onblocked: null };
    setTimeout(() => {
      if (this.failing) return req.onerror?.();
      const fresh = this.stores.size === 0;
      req.result = new FakeDB(this);
      if (fresh) req.onupgradeneeded?.();
      req.onsuccess?.();
    }, 0);
    return req;
  }
}

class FakeDB {
  constructor(private idb: FakeIDB) {}
  objectStoreNames = { contains: (n: string) => this.idb.stores.has(n) };
  createObjectStore(n: string) {
    this.idb.stores.set(n, new Map());
  }
  close() {}
  transaction(name: string) {
    const data = this.idb.stores.get(name);
    if (!data) throw new Error("NotFoundError");
    const tx = new FakeTx(data);
    // A transaction with no request completes too.
    tx.kick();
    return tx;
  }
}

class FakeTx {
  oncomplete: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onabort: (() => void) | null = null;
  private queue: (() => void)[] = [];
  private running = false;
  constructor(private data: Map<string, Value>) {}
  objectStore() {
    const req = <T,>(run: () => T): Req<T> => {
      const r: Req<T> = { result: undefined as T, onsuccess: null, onerror: null };
      this.queue.push(() => {
        r.result = run();
        r.onsuccess?.();
      });
      this.kick();
      return r;
    };
    const copy = (v: Value) => (v === undefined ? undefined : structuredClone(v));
    return {
      get: (k: string) => req(() => copy(this.data.get(k))),
      put: (v: Value, k: string) => req(() => void this.data.set(k, copy(v))),
      delete: (k: string) => req(() => void this.data.delete(k)),
      getAllKeys: () => req(() => [...this.data.keys()].sort()),
    };
  }
  kick() {
    if (this.running) return;
    this.running = true;
    setTimeout(() => {
      while (this.queue.length) this.queue.shift()!();
      this.oncomplete?.();
    }, 0);
  }
}

const NEW_SKETCH = '{"op":"new_sketch","origin":[0,0,0],"normal":[0,0,1]}';
/** A log of `n` steps (a rectangle drawn n - 1 times), as the kernel keeps it. */
const log = (n: number, tag = 0) => `[${[NEW_SKETCH, ...Array.from({ length: n - 1 }, (_, i) => `{"op":"add_rectangle","sketch":1,"x":${tag},"y":${i},"w":1,"h":1}`)].join(",")}]`;
const rec = (savedAt: number, steps: number, json: string, worth = true) => ({ savedAt, solids: 1, steps, bytes: json.length, worth });

let idb: FakeIDB;
const store = () => idb.stores.get("autosave")!;
/** What an older build (or an earlier start) left in the store. */
function plant(records: Record<string, Value>) {
  if (!idb.stores.has("autosave")) idb.stores.set("autosave", new Map());
  for (const [k, v] of Object.entries(records)) store().set(k, v);
}
const ids = (o: { offers: Offer[] }) => o.offers.map((x) => x.id);
const keys = () => [...store().keys()].sort();

describe("unsaved work set aside (lane 64, #94)", () => {
  beforeEach(() => {
    idb = new FakeIDB();
    vi.stubGlobal("indexedDB", idb);
  });
  afterEach(() => vi.unstubAllGlobals());

  it("the issue's three starts: the newer work is offered first, and the older stays set aside", async () => {
    const a = log(3, 1);
    const b = log(4, 2);
    // Start 1 draws A and closes without saving.
    await saveAutosave(a, { savedAt: 1000, solids: 1, steps: 3, worth: true });
    // Start 2: A is offered and left unanswered; B is drawn and the window closed.
    expect(ids(await takeAutosaves())).toEqual(["offered"]);
    await saveAutosave(b, { savedAt: 2000, solids: 1, steps: 4, worth: true });
    // Start 3: B first (before lane 64, A again and B never).
    const found = await takeAutosaves();
    expect(found.offers.map((o) => [o.id, o.steps, o.savedAt])).toEqual([
      ["offered-2", 4, 2000],
      ["offered", 3, 1000],
    ]);
    expect(found.dropped).toEqual([]);
    expect(await loadOffered("offered-2")).toBe(b);
    expect(await loadOffered("offered")).toBe(a);
    // Nothing of this session's is kept yet: the first edit cannot overwrite either.
    expect(keys()).toEqual(["offered", "offered-2", "offered-2-meta", "offered-meta"]);
  });

  it("Restore makes the offer this session's autosave and leaves the other offers; Discard deletes only its own", async () => {
    plant({ offered: log(3, 1), "offered-meta": rec(1000, 3, log(3, 1)), "offered-2": log(4, 2), "offered-2-meta": rec(2000, 4, log(4, 2)) });
    expect(await adoptOffered("offered-2")).toBe(true);
    expect(store().get("current")).toBe(log(4, 2));
    expect((store().get("current-meta") as { steps: number }).steps).toBe(4);
    expect(keys()).toEqual(["current", "current-meta", "offered", "offered-meta"]);
    // The next start offers the restored work (this session's now) and the older work.
    expect(ids(await takeAutosaves())).toEqual(["offered-2", "offered"]);
    await clearOffered("offered");
    expect(keys()).toEqual(["offered-2", "offered-2-meta"]);
    // An offer that is not there is not adopted, and nothing is written.
    expect(await adoptOffered("offered-9")).toBe(false);
    expect(keys()).toEqual(["offered-2", "offered-2-meta"]);
  });

  it("Restore over unsaved work in this session sets that work aside first", async () => {
    plant({ offered: log(3, 1), "offered-meta": rec(1000, 3, log(3, 1)) });
    // This session's own work, autosaved, then set aside as Restore starts.
    await saveAutosave(log(5, 3), { savedAt: 3000, solids: 2, steps: 5, worth: true });
    const kept = await setAside(log(5, 3), { savedAt: 3000, solids: 2, steps: 5, worth: true });
    expect(kept).toEqual({ id: "offered-2", savedAt: 3000, solids: 2, steps: 5, bytes: log(5, 3).length, worth: true, state: "ok" });
    expect(await adoptOffered("offered")).toBe(true);
    expect(store().get("current")).toBe(log(3, 1));
    expect(await loadOffered("offered-2")).toBe(log(5, 3));
    // A log over the limit cannot be set aside (the app asks before it is replaced).
    expect(await setAside("x".repeat(MAX_AUTOSAVE_BYTES + 1), { savedAt: 1, solids: 0, steps: 1, worth: true })).toBeNull();
  });

  it("setting the open model aside moves it out of this session's autosave: a reload before Restore ends finds it once", async () => {
    await saveAutosave(log(4, 3), { savedAt: 3000, solids: 1, steps: 4, worth: true });
    const kept = await setAside(log(5, 3), { savedAt: 3100, solids: 1, steps: 5, worth: true });
    expect(kept?.id).toBe("offered");
    expect(keys()).toEqual(["offered", "offered-meta"]);
    // The page goes here (a reload, a crash): the next start offers the model once.
    expect((await takeAutosaves()).offers.map((o) => [o.id, o.steps])).toEqual([["offered", 5]]);
    // Not kept (too large): this session's autosave stays as it is.
    await saveAutosave(log(4, 3), { savedAt: 3000, solids: 1, steps: 4, worth: true });
    expect(await setAside("x".repeat(MAX_AUTOSAVE_BYTES + 1), { savedAt: 1, solids: 0, steps: 1, worth: true })).toBeNull();
    expect(keys()).toContain("current");
  });

  it("an exact copy of a log in two pairs is kept once and counts once toward the cap; logs that differ are all kept", async () => {
    // Four real sets, then this session's work in current and a copy of it set aside (what a
    // Restore cut short left before the set-aside was a move).
    for (let n = 1; n <= 4; n++) plant({ [n === 1 ? "offered" : `offered-${n}`]: log(3, n), [`${n === 1 ? "offered" : `offered-${n}`}-meta`]: rec(1000 * n, 3, log(3, n)) });
    plant({ "offered-5": log(5, 9), "offered-5-meta": rec(5000, 5, log(5, 9)), current: log(5, 9), "current-meta": rec(5100, 5, log(5, 9)) });
    const found = await takeAutosaves();
    expect(found.dropped).toEqual([]);
    expect(found.offers.map((o) => [o.id, o.steps])).toEqual([
      ["offered-6", 5],
      ["offered-4", 3],
      ["offered-3", 3],
      ["offered-2", 3],
      ["offered", 3],
    ]);
    expect(keys()).not.toContain("offered-5");
    // Same size and steps, different logs (a rectangle elsewhere): both stay.
    expect(log(3, 1).length).toBe(log(3, 2).length);
    expect((await takeAutosaves()).offers).toHaveLength(5);
  });

  it("Save, Open and New (clearAutosave) and this session's autosaves never touch an offer", async () => {
    plant({ offered: log(3, 1), "offered-meta": rec(1000, 3, log(3, 1)) });
    await saveAutosave(log(4, 2), { savedAt: 2000, solids: 1, steps: 4, worth: true });
    await clearAutosave();
    expect(keys()).toEqual(["offered", "offered-meta"]);
  });

  it("reads what older builds left: current only, offered only, both, and a pre-lane-16 record", async () => {
    // current only (any build).
    plant({ current: log(3, 1), "current-meta": rec(1000, 3, log(3, 1)) });
    let found = await takeAutosaves();
    expect(found.offers.map((o) => [o.id, o.state, o.steps])).toEqual([["offered", "ok", 3]]);
    expect(await loadOffered("offered")).toBe(log(3, 1));

    // offered only (an offer an older build left unanswered).
    idb = new FakeIDB();
    vi.stubGlobal("indexedDB", idb);
    plant({ offered: log(4, 2), "offered-meta": rec(2000, 4, log(4, 2)) });
    found = await takeAutosaves();
    expect(found.offers.map((o) => [o.id, o.steps])).toEqual([["offered", 4]]);
    expect(keys()).toEqual(["offered", "offered-meta"]);

    // both pairs: the newer (current) first, the older offer kept as it is.
    idb = new FakeIDB();
    vi.stubGlobal("indexedDB", idb);
    plant({ offered: log(3, 1), "offered-meta": rec(1000, 3, log(3, 1)), current: log(4, 2), "current-meta": rec(2000, 4, log(4, 2)) });
    found = await takeAutosaves();
    expect(found.offers.map((o) => [o.id, o.steps])).toEqual([
      ["offered-2", 4],
      ["offered", 3],
    ]);
    expect(await loadOffered("offered-2")).toBe(log(4, 2));
    expect(await loadOffered("offered")).toBe(log(3, 1));

    // pre-lane-16: a log array in `current` and no record beside it.
    idb = new FakeIDB();
    vi.stubGlobal("indexedDB", idb);
    const ops = JSON.parse(log(3, 1)) as unknown[];
    plant({ current: { log: ops, savedAt: 1500, solids: 1 } });
    found = await takeAutosaves();
    expect(found.offers).toEqual([{ id: "offered", savedAt: 1500, solids: 1, steps: 3, bytes: JSON.stringify(ops).length, worth: true, state: "ok" }]);
    expect(JSON.parse((await loadOffered("offered"))!)).toEqual(ops);
  });

  it("keeps at most MAX_OFFERS: the oldest goes, and the start says so", async () => {
    expect(MAX_OFFERS).toBe(5);
    plant({ offered: log(3, 1), "offered-meta": rec(1000, 3, log(3, 1)) });
    for (let n = 2; n <= 5; n++) plant({ [`offered-${n}`]: log(3, n), [`offered-${n}-meta`]: rec(1000 * n, 3, log(3, n)) });
    await saveAutosave(log(6, 6), { savedAt: 6000, solids: 1, steps: 6, worth: true });
    const found = await takeAutosaves();
    expect(ids(found)).toEqual(["offered-6", "offered-5", "offered-4", "offered-3", "offered-2"]);
    expect(found.dropped.map((d) => [d.id, d.savedAt])).toEqual([["offered", 1000]]);
    expect(keys()).not.toContain("offered");
    expect(keys()).not.toContain("offered-meta");
    expect(droppedNote(found.dropped, 1000)).toBe("At most 5 sets of unsaved work are kept: the oldest (1 solid, 3 steps, saved just now) was deleted.");
    expect(droppedNote([], 0)).toBe("");
    // A start with nothing new keeps the five as they are.
    expect((await takeAutosaves()).dropped).toEqual([]);
  });

  it("a log that holds no work is not set aside; an unreadable record is offered last, for discarding", async () => {
    plant({ current: log(1), "current-meta": rec(5000, 1, log(1), false), "offered-3": log(1), "offered-3-meta": rec(4000, 1, log(1), false) });
    expect(await takeAutosaves()).toEqual({ offers: [], dropped: [] });
    expect(keys()).toEqual([]);
    plant({ offered: 12345, "offered-meta": "garbage", current: log(3, 1), "current-meta": rec(1000, 3, log(3, 1)) });
    const found = await takeAutosaves();
    expect(found.offers.map((o) => [o.id, o.state])).toEqual([
      ["offered-2", "ok"],
      ["offered", "unreadable"],
    ]);
    expect(await loadOffered("offered")).toBeNull();
  });

  it("storage that fails is silent and never blocks the start", async () => {
    plant({ current: log(3, 1), "current-meta": rec(1000, 3, log(3, 1)) });
    idb.failing = true;
    expect(await takeAutosaves()).toEqual({ offers: [], dropped: [] });
    expect(await saveAutosave(log(3, 1), { savedAt: 1, solids: 1, steps: 3, worth: true })).toBe(true);
    expect(await setAside(log(3, 1), { savedAt: 1, solids: 1, steps: 3, worth: true })).toBeNull();
    expect(await adoptOffered("offered")).toBe(false);
    expect(await loadOffered("offered")).toBeNull();
    await clearOffered("offered");
    await clearAutosave();
    // Nothing was lost meanwhile.
    expect(keys()).toEqual(["current", "current-meta"]);
  });

  it("names offers and orders them", () => {
    expect(freeOfferId([])).toBe("offered");
    expect(freeOfferId(["current", "current-meta"])).toBe("offered");
    expect(freeOfferId(["offered", "offered-meta"])).toBe("offered-2");
    expect(freeOfferId(["offered-meta"])).toBe("offered-2");
    expect(freeOfferId(["offered-3", "offered-3-meta"])).toBe("offered");
    expect(freeOfferId(["offered", "offered-meta", "offered-7-meta", 3])).toBe("offered-8");
    const o = (id: string, savedAt: number, state: Autosave["state"] = "ok"): Offer => ({ id, ...meta({ savedAt, state }) });
    expect([o("offered", 1), o("offered-2", 3), o("offered-3", 0, "unreadable"), o("offered-4", 3), o("offered-5", 2, "too_large")].sort(newestFirst).map((x) => x.id)).toEqual([
      "offered-4",
      "offered-2",
      "offered-5",
      "offered",
      "offered-3",
    ]);
    // An id that is not an offer's is never read or deleted.
    expect(freeOfferId(["current"])).toBe("offered");
  });

  it("says how much is set aside", () => {
    expect(olderNote(1)).toBe("1 older set of unsaved work is set aside too.");
    expect(olderNote(3)).toBe("3 older sets of unsaved work are set aside too.");
    expect(setAsideNote(1)).toBe("Unsaved work set aside. Click to restore or discard.");
    expect(setAsideNote(2)).toBe("Unsaved work set aside (2). Click to restore or discard.");
  });

  it("refuses ids that are not an offer's", async () => {
    plant({ current: log(3, 1), "current-meta": rec(1000, 3, log(3, 1)) });
    expect(await loadOffered("current")).toBeNull();
    await clearOffered("current");
    expect(await adoptOffered("current")).toBe(false);
    expect(keys()).toEqual(["current", "current-meta"]);
  });
});
