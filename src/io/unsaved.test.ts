// #120: the question over unsaved changes has three answers, Save, Discard Changes and
// Cancel. This is the question without its looks (src/io/unsaved.ts); the dialog is tested
// in the browser (e2e/unsaved.spec.ts), and what the desktop window does with each answer
// in closing.test.ts.
import { afterEach, describe, expect, it, vi } from "vitest";
import { goesAhead, SAVING_BEAT_MS, UNSAVED_LABELS, unsavedQuestion, unsavedQuestionText, type AskOptions, type SaveResult, type UnsavedOutcome, type UnsavedPrompt } from "./unsaved";

/** The question with a dialog the test presses and a save the test ends. */
function setup(over: { name?: string | null; show?: (p: UnsavedPrompt | null) => void; realTimer?: boolean } = {}) {
  const shown: (UnsavedPrompt | null)[] = [];
  let dirty = true;
  let beats = 0;
  let tick: (() => void) | null = null;
  const saves: { signal: AbortSignal; queued: boolean; end(r: SaveResult): void; fail(e: Error): void }[] = [];
  const q = unsavedQuestion({
    unsaved: () => dirty,
    name: () => over.name ?? null,
    show: (p) => {
      over.show?.(p);
      shown.push(p);
    },
    save: ({ signal, queued }) =>
      new Promise<SaveResult>((resolve, reject) => {
        saves.push({
          signal,
          queued,
          end(r) {
            if (r === "saved") dirty = false;
            resolve(r);
          },
          fail: reject,
        });
      }),
    beat: () => void beats++,
    every: over.realTimer
      ? undefined
      : (fn, ms) => {
          expect(ms).toBe(SAVING_BEAT_MS);
          tick = fn;
          return () => (tick = null);
        },
  });
  return {
    q,
    saves,
    shown,
    /** The dialog as it is now. */
    prompt: () => shown.at(-1) ?? null,
    beats: () => beats,
    tick: () => tick?.(),
    beating: () => tick !== null,
    clean: () => (dirty = false),
    ask: (a?: AskOptions) => {
      const p = q.ask(a);
      let out: UnsavedOutcome | "pending" = "pending";
      void p.then((o) => (out = o));
      return { done: p, out: () => out };
    },
  };
}

/** Let promise chains run. */
const settle = async () => {
  for (let i = 0; i < 5; i++) await Promise.resolve();
};

afterEach(() => vi.useRealTimers());

describe("the question's words", () => {
  it("name the model by its file, or this model when untitled, and say what else happens at a close", () => {
    expect(unsavedQuestionText("cabinet.shz")).toBe("Save the changes to cabinet.shz?");
    expect(unsavedQuestionText(null)).toBe("Save the changes to this model?");
    expect(unsavedQuestionText("cabinet.shz", true)).toBe("Save the changes to cabinet.shz before Sawhorse closes?");
    expect(unsavedQuestionText(null, true)).toBe("Save the changes to this model before Sawhorse closes?");
  });

  it("offer Save, Discard Changes and Cancel, in that order", () => {
    expect(Object.entries(UNSAVED_LABELS)).toEqual([
      ["save", "Save"],
      ["discard", "Discard Changes"],
      ["cancel", "Cancel"],
    ]);
  });

  it("the eighth of a patience: the shell hears saving well inside its two seconds even from a timer slowed to once a second", () => {
    expect(SAVING_BEAT_MS).toBeLessThanOrEqual(250);
  });
});

describe("what goes ahead", () => {
  it("is everything but Cancel and a save that did not happen", () => {
    expect((["clean", "saved", "discarded", "cancelled", "not-saved"] as const).map(goesAhead)).toEqual([true, true, true, false, false]);
  });
});

describe("the question", () => {
  it("is not asked when nothing is unsaved", async () => {
    const t = setup();
    t.clean();
    await expect(t.q.ask()).resolves.toBe("clean");
    expect(t.shown).toEqual([]);
    expect(t.q.up()).toBe(false);
  });

  it("is asked all the same when the caller says always (Restore: the log says there is work)", async () => {
    const t = setup();
    t.clean();
    const a = t.ask({ always: true });
    await settle();
    expect(t.prompt()).toMatchObject({ question: "Save the changes to this model?", saving: null });
    t.prompt()?.choose("discard");
    await expect(a.done).resolves.toBe("discarded");
  });

  it("shows the three answers and waits", async () => {
    const t = setup({ name: "cabinet.shz" });
    const a = t.ask();
    await settle();
    expect(t.prompt()).toMatchObject({ question: "Save the changes to cabinet.shz?", saving: null });
    expect(a.out()).toBe("pending");
    expect(t.q.up()).toBe(true);
    expect(t.saves).toEqual([]);
    expect(t.beats()).toBe(0);
  });

  it("Cancel: the dialog goes, nothing is saved", async () => {
    const t = setup();
    const a = t.ask();
    await settle();
    t.prompt()?.choose("cancel");
    await expect(a.done).resolves.toBe("cancelled");
    expect(t.prompt()).toBeNull();
    expect(t.saves).toEqual([]);
    expect(t.q.up()).toBe(false);
  });

  it("Discard Changes: the dialog goes, nothing is saved", async () => {
    const t = setup();
    const a = t.ask();
    await settle();
    t.prompt()?.choose("discard");
    await expect(a.done).resolves.toBe("discarded");
    expect(t.prompt()).toBeNull();
    expect(t.saves).toEqual([]);
  });

  it("Save: runs the save, shows Saving with the name, and ends saved when it worked", async () => {
    const t = setup({ name: "cabinet.shz" });
    const a = t.ask();
    await settle();
    t.prompt()?.choose("save");
    expect(t.prompt()).toMatchObject({ question: "Save the changes to cabinet.shz?", saving: "Saving cabinet.shz…" });
    expect(t.saves).toHaveLength(1);
    expect(t.saves[0].queued).toBe(false);
    expect(t.saves[0].signal.aborted).toBe(false);
    await settle();
    expect(a.out()).toBe("pending");
    t.saves[0].end("saved");
    await expect(a.done).resolves.toBe("saved");
    expect(t.prompt()).toBeNull();
    expect(t.q.up()).toBe(false);
  });

  it("an untitled model saves as Saving…", async () => {
    const t = setup();
    void t.ask();
    await settle();
    t.prompt()?.choose("save");
    expect(t.prompt()?.saving).toBe("Saving…");
  });

  for (const how of ["cancelled", "failed"] as const) {
    it(`Save, and the save ${how}: ends as not saved, the dialog gone`, async () => {
      const t = setup();
      const a = t.ask();
      await settle();
      t.prompt()?.choose("save");
      t.saves[0].end(how);
      await expect(a.done).resolves.toBe("not-saved");
      expect(t.prompt()).toBeNull();
    });
  }

  it("a save that throws counts as failed", async () => {
    const t = setup();
    const a = t.ask();
    await settle();
    t.prompt()?.choose("save");
    t.saves[0].fail(new Error("disk full"));
    await expect(a.done).resolves.toBe("not-saved");
  });

  it("a question asked from inside the edit queue tells the save so", async () => {
    const t = setup();
    void t.ask({ queued: true });
    await settle();
    t.prompt()?.choose("save");
    expect(t.saves[0].queued).toBe(true);
  });

  it("an answer counts once: a second click does nothing", async () => {
    const t = setup();
    const a = t.ask();
    await settle();
    const p = t.prompt();
    p?.choose("save");
    p?.choose("save");
    p?.choose("discard");
    expect(t.saves).toHaveLength(1);
    t.saves[0].end("saved");
    await expect(a.done).resolves.toBe("saved");
    p?.choose("cancel");
    p?.choose("discard");
    expect(t.shown.at(-1)).toBeNull();
  });
});

describe("saving, as the shell hears it", () => {
  it("beats at once on Save and at each tick until the save has ended, then stops", async () => {
    const t = setup();
    const a = t.ask();
    await settle();
    expect(t.beating()).toBe(false);
    t.prompt()?.choose("save");
    expect(t.beats()).toBe(1);
    expect(t.beating()).toBe(true);
    t.tick();
    t.tick();
    t.tick();
    expect(t.beats()).toBe(4);
    t.saves[0].end("saved");
    await a.done;
    expect(t.beating()).toBe(false);
  });

  it("does not beat for Cancel or Discard Changes", async () => {
    for (const c of ["cancel", "discard"] as const) {
      const t = setup();
      const a = t.ask();
      await settle();
      t.prompt()?.choose(c);
      await a.done;
      expect(t.beats()).toBe(0);
      expect(t.beating()).toBe(false);
    }
  });

  it("stops when the save is cancelled or fails", async () => {
    for (const how of ["cancelled", "failed"] as const) {
      const t = setup();
      const a = t.ask();
      await settle();
      t.prompt()?.choose("save");
      t.saves[0].end(how);
      await a.done;
      expect(t.beating()).toBe(false);
    }
  });

  it("with the real timer: every SAVING_BEAT_MS, and none after the end", async () => {
    vi.useFakeTimers();
    const t = setup({ realTimer: true });
    const a = t.ask();
    await vi.advanceTimersByTimeAsync(0);
    t.prompt()?.choose("save");
    expect(t.beats()).toBe(1);
    await vi.advanceTimersByTimeAsync(SAVING_BEAT_MS * 4);
    expect(t.beats()).toBe(5);
    // A minute in a file dialog: about eight a second.
    await vi.advanceTimersByTimeAsync(60_000);
    expect(t.beats()).toBe(5 + 60_000 / SAVING_BEAT_MS);
    t.saves[0].end("cancelled");
    await a.done;
    const at = t.beats();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(t.beats()).toBe(at);
  });

  it("a beat that throws does not stop the save or the question", async () => {
    let n = 0;
    const q = unsavedQuestion({
      unsaved: () => true,
      name: () => null,
      show: (p) => p?.saving === null && queueMicrotask(() => p.choose("save")),
      save: async () => "saved",
      beat: () => {
        n++;
        throw new Error("no shell");
      },
      every: () => () => {},
    });
    await expect(q.ask()).resolves.toBe("saved");
    expect(n).toBe(1);
  });
});

describe("Cancel while a save runs", () => {
  it("gives up on it: the signal is aborted, the beat stops, the dialog goes", async () => {
    const t = setup();
    const a = t.ask();
    await settle();
    t.prompt()?.choose("save");
    const saving = t.prompt();
    // Only Cancel does anything while saving.
    saving?.choose("save");
    saving?.choose("discard");
    await settle();
    expect(a.out()).toBe("pending");
    expect(t.saves).toHaveLength(1);
    saving?.choose("cancel");
    await expect(a.done).resolves.toBe("not-saved");
    expect(t.saves[0].signal.aborted).toBe(true);
    expect(t.beating()).toBe(false);
    expect(t.prompt()).toBeNull();
    expect(t.q.up()).toBe(false);
  });

  it("the save ending afterwards changes nothing, and a new question can be asked meanwhile", async () => {
    const t = setup();
    const a = t.ask();
    await settle();
    t.prompt()?.choose("save");
    t.prompt()?.choose("cancel");
    await a.done;
    const b = t.ask();
    await settle();
    expect(t.prompt()?.saving).toBeNull();
    t.saves[0].end("cancelled");
    await settle();
    expect(b.out()).toBe("pending");
    expect(t.prompt()?.saving).toBeNull();
    t.prompt()?.choose("discard");
    await expect(b.done).resolves.toBe("discarded");
  });
});

describe("one question at a time", () => {
  it("a second question while the first shows its answers takes its place: the first ends as Cancel", async () => {
    const t = setup({ name: "cabinet.shz" });
    const first = t.ask();
    await settle();
    const second = t.ask({ closing: true });
    await expect(first.done).resolves.toBe("cancelled");
    await settle();
    expect(second.out()).toBe("pending");
    expect(t.prompt()).toMatchObject({ question: "Save the changes to cabinet.shz before Sawhorse closes?", saving: null });
    // The first question's buttons are dead.
    t.shown[0]?.choose("discard");
    await settle();
    expect(second.out()).toBe("pending");
    t.prompt()?.choose("cancel");
    await expect(second.done).resolves.toBe("cancelled");
    expect(t.prompt()).toBeNull();
  });

  it("a second question while a save runs waits for it, and is not asked when the save worked", async () => {
    const t = setup();
    const first = t.ask();
    await settle();
    t.prompt()?.choose("save");
    const second = t.ask({ closing: true });
    await settle();
    // Nothing is shown over the save, and the shell still hears of it.
    expect(t.prompt()?.saving).toBe("Saving…");
    expect(t.beating()).toBe(true);
    expect(second.out()).toBe("pending");
    t.saves[0].end("saved");
    await expect(first.done).resolves.toBe("saved");
    await expect(second.done).resolves.toBe("clean");
    expect(t.prompt()).toBeNull();
  });

  it("a second question while a save runs is asked once the save was cancelled", async () => {
    const t = setup();
    const first = t.ask();
    await settle();
    t.prompt()?.choose("save");
    const second = t.ask({ closing: true });
    await settle();
    t.saves[0].end("cancelled");
    await expect(first.done).resolves.toBe("not-saved");
    await settle();
    expect(second.out()).toBe("pending");
    expect(t.prompt()).toMatchObject({ question: "Save the changes to this model before Sawhorse closes?", saving: null });
    t.prompt()?.choose("discard");
    await expect(second.done).resolves.toBe("discarded");
  });
});

describe("a dialog that cannot be shown", () => {
  it("rejects, and the next question is asked afresh", async () => {
    let broken = true;
    const t = setup({
      show: (p) => {
        if (broken && p) throw new Error("no dialog");
      },
    });
    await expect(t.q.ask()).rejects.toThrow("no dialog");
    expect(t.q.up()).toBe(false);
    broken = false;
    const a = t.ask();
    await settle();
    t.prompt()?.choose("cancel");
    await expect(a.done).resolves.toBe("cancelled");
  });

  it("a dialog that cannot be taken away does not lose the answer", async () => {
    const t = setup({
      show: (p) => {
        if (!p) throw new Error("stuck");
      },
    });
    const a = t.ask();
    await settle();
    t.shown.at(-1)?.choose("discard");
    await expect(a.done).resolves.toBe("discarded");
  });
});
