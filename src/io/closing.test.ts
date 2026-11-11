// Lane 35 (#44): the desktop window's X, Alt+F4 and File > Exit ask before unsaved changes
// are lost, and nothing the page does can hold the window open. The shell's side (the two
// second rule) is tested in src-tauri/close-guard; this is the page's.
import { afterEach, describe, expect, it, vi } from "vitest";
import { ANSWER_FLOOR_MS, CLOSE_REQUESTED, closeHandler, type CloseOptions, type CloseShell, type CloseWord, desktopShell, DISCARD_WAIT_MS, discardQuestion } from "./closing";

const dialog = vi.hoisted(() => ({ confirm: vi.fn<(message: string, options?: object) => Promise<boolean>>() }));
const core = vi.hoisted(() => ({ invoke: vi.fn<(cmd: string, args?: unknown) => Promise<void>>() }));
const event = vi.hoisted(() => ({ listen: vi.fn<(name: string, cb: (e: unknown) => void) => Promise<() => void>>() }));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
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

function setup(over: Partial<CloseOptions> & { answer?: boolean; after?: number } = {}) {
  const s = shell();
  const c = clock();
  const asked: string[] = [];
  const failures: string[] = [];
  const discard = vi.fn(async () => {});
  const o: CloseOptions = {
    shell: s,
    unsaved: () => true,
    ask: async (q) => {
      asked.push(q);
      c.pass(over.after ?? 800);
      return over.answer ?? false;
    },
    discard,
    failed: (m) => failures.push(m),
    now: c.now,
    ...over,
  };
  return { handle: closeHandler(o), s, asked, failures, discard, c };
}

afterEach(() => {
  vi.useRealTimers();
  dialog.confirm.mockReset();
  core.invoke.mockReset();
  event.listen.mockReset();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe("the question's words", () => {
  it("are one text: as it is for New, Open and Close, with what happens to the window for closing it", () => {
    expect(discardQuestion()).toBe("Discard the unsaved changes to this model?");
    expect(discardQuestion(true)).toBe("Discard the unsaved changes to this model and close Sawhorse?");
  });
});

describe("a close request in the desktop app", () => {
  it("with nothing unsaved closes without a question", async () => {
    const t = setup({ unsaved: () => false });
    await t.handle();
    expect(t.asked).toEqual([]);
    expect(t.s.said).toEqual(["close"]);
    expect(t.discard).not.toHaveBeenCalled();
  });

  it("with unsaved changes asks, and No keeps the window, the model and the autosave", async () => {
    const t = setup({ answer: false });
    await t.handle();
    expect(t.asked).toEqual(["Discard the unsaved changes to this model and close Sawhorse?"]);
    expect(t.s.said).toEqual(["keep"]);
    expect(t.discard).not.toHaveBeenCalled();
    expect(t.failures).toEqual([]);
  });

  it("with unsaved changes asks, and Yes deletes the autosave as File > New does, then closes", async () => {
    const order: string[] = [];
    const t = setup({ answer: true, discard: async () => void order.push("discard") });
    const say = t.s.say.bind(t.s);
    t.s.say = async (w) => {
      order.push(w);
      await say(w);
    };
    await t.handle();
    expect(order).toEqual(["discard", "close"]);
  });

  it("a second request while the question is up asks nothing more, and the next one after the answer asks again", async () => {
    let answer!: (yes: boolean) => void;
    const asked: string[] = [];
    const t = setup({
      ask: (q) => {
        asked.push(q);
        return new Promise<boolean>((r) => (answer = r));
      },
    });
    const first = t.handle();
    await vi.waitFor(() => expect(asked).toHaveLength(1));
    await t.handle();
    await t.handle();
    expect(asked).toHaveLength(1);
    expect(t.s.said).toEqual([]);
    t.c.pass(ANSWER_FLOOR_MS + 1000);
    answer(false);
    await first;
    expect(t.s.said).toEqual(["keep"]);
    const again = t.handle();
    await vi.waitFor(() => expect(asked).toHaveLength(2));
    t.c.pass(ANSWER_FLOOR_MS + 1000);
    answer(false);
    await again;
    expect(t.s.said).toEqual(["keep", "keep"]);
  });

  it("the question failing to open says so and lets the next close close", async () => {
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

  it("a No sooner than a person could answer means the dialog never showed: failed, not kept", async () => {
    const t = setup({ answer: false, after: ANSWER_FLOOR_MS - 1 });
    await t.handle();
    expect(t.s.said).toEqual(["failed"]);
    expect(t.failures[0]).toContain("the question closed before it could be read");
    // A No at the floor is a person's.
    const u = setup({ answer: false, after: ANSWER_FLOOR_MS });
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
    expect(t.asked).toEqual([]);
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

  it("after Yes the window closes even when deleting the autosave fails or never finishes", async () => {
    const t = setup({ answer: true, discard: async () => Promise.reject(new Error("storage blocked")) });
    await t.handle();
    expect(t.s.said).toEqual(["close"]);

    vi.useFakeTimers();
    const u = setup({ answer: true, discard: () => new Promise<void>(() => {}) });
    const done = u.handle();
    await vi.advanceTimersByTimeAsync(DISCARD_WAIT_MS - 1);
    expect(u.s.said).toEqual([]);
    await vi.advanceTimersByTimeAsync(1);
    await done;
    expect(u.s.said).toEqual(["close"]);
  });

  it("the handler is free again after a failure: the next request asks", async () => {
    let fail = true;
    const t = setup({
      ask: async () => {
        if (fail) throw new Error("no dialog");
        t.c.pass(1000);
        return false;
      },
    });
    await t.handle();
    fail = false;
    await t.handle();
    expect(t.s.said).toEqual(["failed", "keep"]);
  });

  it("by default asks with the desktop's message dialog, titled Sawhorse, and waits for it", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    dialog.confirm.mockImplementation(() => new Promise((r) => setTimeout(() => r(false), 300)));
    const s = shell();
    await closeHandler({ shell: s, unsaved: () => true, discard: async () => {}, failed: () => {} })();
    expect(dialog.confirm).toHaveBeenCalledWith("Discard the unsaved changes to this model and close Sawhorse?", expect.objectContaining({ title: "Sawhorse", kind: "warning" }));
    expect(s.said).toEqual(["keep"]);
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
    for (const w of ["unsaved", "saved", "keep", "close", "failed", "exit"] as const) await sh.say(w);
    expect(core.invoke.mock.calls).toEqual(["unsaved", "saved", "keep", "close", "failed", "exit"].map((say) => ["close_guard", { say }]));
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
