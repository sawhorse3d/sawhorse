// Lane 35 (#44): the desktop window's X, Alt+F4 and File > Exit ask before unsaved changes
// are lost, and nothing the page does can hold the window open. The shell's side (the two
// second rule, and the word "saving") is tested in src-tauri/close-guard; this is the page's.
// Since #120 the question has three answers and is the page's own (src/io/unsaved.ts, tested
// in unsaved.test.ts); here it is stood in for by how it ended.
import { afterEach, describe, expect, it, vi } from "vitest";
import { ANSWER_FLOOR_MS, CLOSE_REQUESTED, closeHandler, type CloseOptions, type CloseShell, type CloseWord, desktopShell, DISCARD_WAIT_MS } from "./closing";
import { unsavedQuestion, type UnsavedOutcome, type UnsavedPrompt } from "./unsaved";

const core = vi.hoisted(() => ({ invoke: vi.fn<(cmd: string, args?: unknown) => Promise<void>>() }));
const event = vi.hoisted(() => ({ listen: vi.fn<(name: string, cb: (e: unknown) => void) => Promise<() => void>>() }));
vi.mock("@tauri-apps/api/core", () => core);
vi.mock("@tauri-apps/api/event", () => event);

/** A shell that writes down what the page says. */
function shell(fail: CloseWord[] = []): CloseShell & { said: CloseWord[] } {
  const said: CloseWord[] = [];
  return {
    said,
    async say(w) {
      said.push(w);
      if (fail.includes(w)) throw new Error(`${w} did not reach the shell`);
    },
    async onRequest() {
      return () => {};
    },
  };
}

/** A clock the test moves. */
function clock() {
  let t = 1000;
  return { now: () => t, pass: (ms: number) => (t += ms) };
}

function setup(over: Partial<CloseOptions> & { answer?: UnsavedOutcome; after?: number } = {}) {
  const s = shell();
  const c = clock();
  let asked = 0;
  const failures: string[] = [];
  const discard = vi.fn(async () => {});
  const o: CloseOptions = {
    shell: s,
    unsaved: () => true,
    ask: async () => {
      asked++;
      c.pass(over.after ?? 800);
      return over.answer ?? "cancelled";
    },
    discard,
    failed: (m) => failures.push(m),
    now: c.now,
    ...over,
  };
  return { handle: closeHandler(o), s, asked: () => asked, failures, discard, c };
}

afterEach(() => {
  vi.useRealTimers();
  core.invoke.mockReset();
  event.listen.mockReset();
});

describe("a close request in the desktop app", () => {
  it("with nothing unsaved closes without a question", async () => {
    const t = setup({ unsaved: () => false });
    await t.handle();
    expect(t.asked()).toBe(0);
    expect(t.s.said).toEqual(["close"]);
    expect(t.discard).not.toHaveBeenCalled();
  });

  it("with unsaved changes asks, and Cancel keeps the window, the model and the autosave", async () => {
    const t = setup({ answer: "cancelled" });
    await t.handle();
    expect(t.asked()).toBe(1);
    expect(t.s.said).toEqual(["keep"]);
    expect(t.discard).not.toHaveBeenCalled();
    expect(t.failures).toEqual([]);
  });

  it("Discard Changes deletes the autosave as File > New does, then closes", async () => {
    const order: string[] = [];
    const t = setup({ answer: "discarded", discard: async () => void order.push("discard") });
    const say = t.s.say.bind(t.s);
    t.s.say = async (w) => {
      order.push(w);
      await say(w);
    };
    await t.handle();
    expect(order).toEqual(["discard", "close"]);
  });

  // A save starts the autosave's delete and does not wait for it: a window closed at once
  // after one kept the autosave, and the next start offered the saved work back as unsaved.
  for (const answer of ["saved", "clean"] as const) {
    it(`answer "${answer}": the autosave's delete is waited for before the window closes`, async () => {
      const order: string[] = [];
      let gone = () => {};
      const t = setup({
        answer,
        discard: () =>
          new Promise<void>((r) => {
            order.push("delete started");
            gone = () => {
              order.push("deleted");
              r();
            };
          }),
      });
      const say = t.s.say.bind(t.s);
      t.s.say = async (w) => {
        order.push(w);
        await say(w);
      };
      const done = t.handle();
      await vi.waitFor(() => expect(order).toEqual(["delete started"]));
      await Promise.resolve();
      expect(order).toEqual(["delete started"]);
      gone();
      await done;
      expect(order).toEqual(["delete started", "deleted", "close"]);
    });
  }

  it("Save, and the save cancelled or failed: the window, the model and the autosave stay, as with Cancel", async () => {
    const t = setup({ answer: "not-saved" });
    await t.handle();
    expect(t.s.said).toEqual(["keep"]);
    expect(t.discard).not.toHaveBeenCalled();
    expect(t.failures).toEqual([]);
    // A save dialog dismissed at once is a person's answer: the floor is for the question.
    const u = setup({ answer: "not-saved", after: 0 });
    await u.handle();
    expect(u.s.said).toEqual(["keep"]);
  });

  it("nothing left unsaved by the time the question would be asked (a save was running): closes", async () => {
    const t = setup({ answer: "clean" });
    await t.handle();
    expect(t.s.said).toEqual(["close"]);
    expect(t.discard).toHaveBeenCalledOnce();
  });

  it("a second request while the question is up asks nothing more, and the next one after the answer asks again", async () => {
    let answer!: (a: UnsavedOutcome) => void;
    let asked = 0;
    const t = setup({
      ask: () => {
        asked++;
        return new Promise<UnsavedOutcome>((r) => (answer = r));
      },
    });
    const first = t.handle();
    await vi.waitFor(() => expect(asked).toBe(1));
    await t.handle();
    await t.handle();
    expect(asked).toBe(1);
    expect(t.s.said).toEqual([]);
    t.c.pass(ANSWER_FLOOR_MS + 1000);
    answer("cancelled");
    await first;
    expect(t.s.said).toEqual(["keep"]);
    const again = t.handle();
    await vi.waitFor(() => expect(asked).toBe(2));
    t.c.pass(ANSWER_FLOOR_MS + 1000);
    answer("cancelled");
    await again;
    expect(t.s.said).toEqual(["keep", "keep"]);
  });

  it("the question failing to show says so and lets the next close close", async () => {
    const t = setup({
      ask: async () => {
        throw new Error("no dialog");
      },
    });
    await t.handle();
    expect(t.s.said).toEqual(["failed"]);
    expect(t.failures).toEqual(["Sawhorse could not ask about the unsaved changes (no dialog). Close the window again to close it without saving, or save first."]);
    expect(t.discard).not.toHaveBeenCalled();
  });

  it("a Cancel sooner than a person could answer means the dialog did not stay up: failed, not kept", async () => {
    const t = setup({ answer: "cancelled", after: ANSWER_FLOOR_MS - 1 });
    await t.handle();
    expect(t.s.said).toEqual(["failed"]);
    expect(t.failures[0]).toContain("the question closed before it could be read");
    // A Cancel at the floor is a person's.
    const u = setup({ answer: "cancelled", after: ANSWER_FLOOR_MS });
    await u.handle();
    expect(u.s.said).toEqual(["keep"]);
  });

  it("the handler throwing (reading the model's state) says failed", async () => {
    const t = setup({
      unsaved: () => {
        throw new Error("no scene");
      },
    });
    await t.handle();
    expect(t.asked()).toBe(0);
    expect(t.s.said).toEqual(["failed"]);
  });

  it("an answer that cannot reach the shell still says failed, and a broken status line does not stop it", async () => {
    const s = shell(["keep"]);
    const t = setup({
      shell: s,
      failed: () => {
        throw new Error("no status line");
      },
    });
    await t.handle();
    expect(s.said).toEqual(["keep", "failed"]);
  });

  for (const answer of ["discarded", "saved"] as const) {
    it(`answer "${answer}": the window closes even when deleting the autosave fails or never finishes`, async () => {
      const t = setup({ answer, discard: async () => Promise.reject(new Error("storage blocked")) });
      await t.handle();
      expect(t.s.said).toEqual(["close"]);

      vi.useFakeTimers();
      const u = setup({ answer, discard: () => new Promise<void>(() => {}) });
      const done = u.handle();
      await vi.advanceTimersByTimeAsync(DISCARD_WAIT_MS - 1);
      expect(u.s.said).toEqual([]);
      await vi.advanceTimersByTimeAsync(1);
      await done;
      expect(u.s.said).toEqual(["close"]);
    });
  }

  it("the handler is free again after a failure: the next request asks", async () => {
    let fail = true;
    const t = setup({
      ask: async () => {
        if (fail) throw new Error("no dialog");
        t.c.pass(1000);
        return "cancelled";
      },
    });
    await t.handle();
    fail = false;
    await t.handle();
    expect(t.s.said).toEqual(["failed", "keep"]);
  });
});

describe("a close request with the question itself (src/io/unsaved.ts)", () => {
  /** The handler over the real question, a dialog the test presses, a save the test ends, and the shell told "saving". */
  function wired(over: { name?: string | null } = {}) {
    const s = shell();
    const c = clock();
    let prompt: UnsavedPrompt | null = null;
    let dirty = true;
    let endSave!: (r: "saved" | "cancelled" | "failed") => void;
    const saves: boolean[] = [];
    let beats: (() => void) | null = null;
    const q = unsavedQuestion({
      unsaved: () => dirty,
      name: () => over.name ?? null,
      show: (p) => (prompt = p),
      save: ({ queued }) => {
        saves.push(queued);
        return new Promise((r) => {
          endSave = (how) => {
            if (how === "saved") dirty = false;
            r(how);
          };
        });
      },
      beat: () => void s.say("saving"),
      every: (fn) => {
        beats = fn;
        return () => (beats = null);
      },
    });
    const discard = vi.fn(async () => {});
    const failures: string[] = [];
    const handle = closeHandler({ shell: s, unsaved: () => dirty, ask: () => q.ask({ closing: true }), discard, failed: (m) => failures.push(m), now: c.now });
    return { s, c, q, handle, discard, failures, saves, prompt: () => prompt, endSave: (r: "saved" | "cancelled" | "failed") => endSave(r), tick: () => beats?.(), beating: () => beats !== null };
  }

  it("asks with what else happens, by the model's name", async () => {
    const t = wired({ name: "cabinet.shz" });
    void t.handle();
    await vi.waitFor(() => expect(t.prompt()).not.toBeNull());
    expect(t.prompt()?.question).toBe("Save the changes to cabinet.shz before Sawhorse closes?");
    expect(t.prompt()?.saving).toBeNull();
  });

  it("Save: says saving at once and at each beat until the save has ended, then closes", async () => {
    const t = wired();
    const done = t.handle();
    await vi.waitFor(() => expect(t.prompt()).not.toBeNull());
    t.c.pass(900);
    t.prompt()?.choose("save");
    expect(t.s.said).toEqual(["saving"]);
    expect(t.prompt()?.saving).toBe("Saving…");
    t.tick();
    t.tick();
    expect(t.s.said).toEqual(["saving", "saving", "saving"]);
    expect(t.saves).toEqual([false]);
    t.endSave("saved");
    await done;
    expect(t.beating()).toBe(false);
    expect(t.s.said).toEqual(["saving", "saving", "saving", "close"]);
    expect(t.discard).toHaveBeenCalledOnce();
    expect(t.prompt()).toBeNull();
  });

  for (const how of ["cancelled", "failed"] as const) {
    it(`Save, and the save ${how}: stops saying saving and keeps the window`, async () => {
      const t = wired();
      const done = t.handle();
      await vi.waitFor(() => expect(t.prompt()).not.toBeNull());
      t.prompt()?.choose("save");
      t.tick();
      t.endSave(how);
      await done;
      expect(t.beating()).toBe(false);
      expect(t.s.said).toEqual(["saving", "saving", "keep"]);
      expect(t.failures).toEqual([]);
      expect(t.prompt()).toBeNull();
    });
  }

  it("Cancel while the save runs gives up on it: no more saving, and keep", async () => {
    const t = wired();
    const done = t.handle();
    await vi.waitFor(() => expect(t.prompt()).not.toBeNull());
    t.prompt()?.choose("save");
    t.prompt()?.choose("cancel");
    await done;
    expect(t.beating()).toBe(false);
    expect(t.s.said).toEqual(["saving", "keep"]);
    // The save ending afterwards says nothing more to the shell.
    t.endSave("cancelled");
    await Promise.resolve();
    expect(t.s.said).toEqual(["saving", "keep"]);
  });

  it("Discard Changes deletes the autosave and closes; Cancel keeps", async () => {
    const t = wired();
    let done = t.handle();
    await vi.waitFor(() => expect(t.prompt()).not.toBeNull());
    t.c.pass(900);
    t.prompt()?.choose("cancel");
    await done;
    expect(t.s.said).toEqual(["keep"]);
    done = t.handle();
    await vi.waitFor(() => expect(t.prompt()).not.toBeNull());
    t.prompt()?.choose("discard");
    await done;
    expect(t.discard).toHaveBeenCalledOnce();
    expect(t.s.said).toEqual(["keep", "close"]);
  });
});

describe("the desktop shell", () => {
  it("speaks through the close_guard command and hears sawhorse://close-requested", async () => {
    core.invoke.mockResolvedValue(undefined);
    const off = vi.fn();
    let heard: ((e: unknown) => void) | undefined;
    event.listen.mockImplementation(async (_name, cb) => {
      heard = cb;
      return off;
    });
    const sh = desktopShell();
    const words = ["unsaved", "saved", "keep", "close", "saving", "failed", "exit"] as const;
    for (const w of words) await sh.say(w);
    expect(core.invoke.mock.calls).toEqual(words.map((say) => ["close_guard", { say }]));
    const cb = vi.fn();
    expect(await sh.onRequest(cb)).toBe(off);
    expect(event.listen.mock.calls[0][0]).toBe(CLOSE_REQUESTED);
    expect(CLOSE_REQUESTED).toBe("sawhorse://close-requested");
    heard?.({ event: CLOSE_REQUESTED, id: 1, payload: null });
    expect(cb).toHaveBeenCalledOnce();
  });

  it("a refused command reaches the caller", async () => {
    core.invoke.mockRejectedValue(new Error("no shell"));
    await expect(desktopShell().say("exit")).rejects.toThrow("no shell");
  });
});
