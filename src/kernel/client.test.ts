// Lane 16 (D1-D3): the kernel client keeps the document's op log, and replaces a worker
// that traps, dies or hangs, rebuilding the document in a fresh one; a worker that keeps
// failing is given up on, and later calls are refused at once instead of waiting forever.
// Driven with a stand-in worker; the real one is driven in e2e/containment.spec.ts.
import { afterEach, describe, expect, it, vi } from "vitest";
import { DEADLINES, KernelClient, KernelRestarted, KernelUnavailableError, type RestartInfo } from "./client";
import type { SceneData, WorkerRequest, WorkerResponse } from "./types";

const scene = { state: { log_len: 0 } } as unknown as SceneData;

class FakeWorker {
  onmessage: ((e: MessageEvent<WorkerResponse>) => void) | null = null;
  onerror: ((e: ErrorEvent) => void) | null = null;
  posted: WorkerRequest[] = [];
  terminated = false;
  postMessage(m: WorkerRequest) {
    this.posted.push(m);
  }
  terminate() {
    this.terminated = true;
  }
  /** Answer the last request. */
  answer(r: Partial<WorkerResponse> & { ok: boolean }) {
    const id = this.posted[this.posted.length - 1].id;
    this.onmessage?.({ data: { id, ...r } as WorkerResponse } as MessageEvent<WorkerResponse>);
  }
  last(): WorkerRequest {
    return this.posted[this.posted.length - 1];
  }
}

function client() {
  const made: FakeWorker[] = [];
  const c = new KernelClient(() => {
    const w = new FakeWorker();
    made.push(w);
    return w as unknown as Worker;
  });
  const restarts: RestartInfo[] = [];
  c.onRestart = (i) => restarts.push(i);
  return { c, made, restarts, w: () => made[made.length - 1] };
}

const flush = async () => {
  for (let i = 0; i < 10; i++) await Promise.resolve();
};

/** Apply an op that the worker accepts, logging `text`. */
async function applied(k: ReturnType<typeof client>, text: string) {
  const p = k.c.apply({ op: "noop" } as never);
  await flush();
  const known = k.w().last().known ?? 0;
  k.w().answer({ ok: true, outcome: {} as never, scene, logSync: { from: known, tail: [text] } });
  await p;
}

afterEach(() => {
  vi.useRealTimers();
});

describe("the kernel client", () => {
  it("keeps the op log from the tails the worker sends, and starts over when told to", async () => {
    const k = client();
    await applied(k, '{"op":"a"}');
    await applied(k, '{"op":"b"}');
    expect(k.c.logTexts()).toEqual(['{"op":"a"}', '{"op":"b"}']);
    expect(k.w().last().known).toBe(1);
    // A compacted log comes back whole, from 0.
    const p = k.c.apply({ op: "noop" } as never);
    await flush();
    k.w().answer({ ok: true, outcome: {} as never, scene, logSync: { from: 0, tail: ['{"op":"restore"}'] } });
    await p;
    expect(k.c.logJson()).toBe('[{"op":"restore"}]');
  });

  it("a trap refuses the request, starts a fresh worker and rebuilds the document from the log", async () => {
    const k = client();
    await applied(k, '{"op":"a"}');
    const first = k.w();
    const p = k.c.importFile("x.stl", new Uint8Array(4));
    await flush();
    first.answer({ ok: false, error: "RuntimeError: unreachable", fatal: true });
    await expect(p).rejects.toBeInstanceOf(KernelRestarted);
    expect(first.terminated).toBe(true);
    expect(k.made).toHaveLength(2);
    // The fresh worker is given the log to replay; a request made meanwhile waits for it.
    const replay = k.w().last();
    expect(replay).toMatchObject({ kind: "replay", json: '[{"op":"a"}]' });
    const waiting = k.c.version();
    await flush();
    expect(k.w().posted).toHaveLength(1);
    k.w().answer({ ok: true, scene });
    await flush();
    expect(k.restarts).toEqual([{ reason: "RuntimeError: unreachable", scene }]);
    expect(k.w().last().kind).toBe("version");
    k.w().answer({ ok: true, version: "1" });
    await expect(waiting).resolves.toBe("1");
  });

  it("a worker that dies (a JavaScript error escaping it) is replaced, and nothing is left waiting", async () => {
    const k = client();
    const p = k.c.version();
    await flush();
    k.w().onerror?.({ message: "out of memory", preventDefault() {} } as unknown as ErrorEvent);
    await expect(p).rejects.toBeInstanceOf(KernelRestarted);
    // No log yet: the fresh worker starts a new document.
    expect(k.w().last().kind).toBe("init");
  });

  it("a request past its deadline is taken for hung: the worker is replaced", async () => {
    vi.useFakeTimers();
    const k = client();
    const p = k.c.importFile("big.obj", new Uint8Array(4));
    const caught = p.catch((e: Error) => e);
    await vi.advanceTimersByTimeAsync(DEADLINES.importFile + 1);
    expect(await caught).toBeInstanceOf(KernelRestarted);
    expect((await caught as Error).message).toContain("importFile ran for more than 180 s");
    expect(k.made[0].terminated).toBe(true);
  });

  it("Cancel replaces the worker the same way", async () => {
    const k = client();
    const p = k.c.openShz(new Uint8Array(4));
    await flush();
    void k.c.cancel("cancelled by you");
    await expect(p).rejects.toThrow("cancelled by you");
    expect(k.made).toHaveLength(2);
  });

  it("gives up after more than three restarts in half a minute; later calls are refused at once", async () => {
    const k = client();
    for (let i = 0; i < 4; i++) {
      const w = k.w();
      w.onerror?.({ message: "will not load", preventDefault() {} } as unknown as ErrorEvent);
      await flush();
      // Each rebuild fails too: the fresh worker dies before answering.
      if (i < 3) {
        k.w().onerror?.({ message: "will not load", preventDefault() {} } as unknown as ErrorEvent);
        await flush();
      }
    }
    await flush();
    const last = k.restarts[k.restarts.length - 1];
    expect(last.scene).toBeNull();
    expect(last.error).toMatch(/failed \d times in half a minute/);
    await expect(k.c.version()).rejects.toBeInstanceOf(KernelUnavailableError);
  });

  it("a fresh worker that dies while rebuilding is replaced in turn, and the rebuild finishes", async () => {
    const k = client();
    await applied(k, '{"op":"a"}');
    k.w().onerror?.({ message: "boom", preventDefault() {} } as unknown as ErrorEvent);
    await flush();
    expect(k.w().last().kind).toBe("replay");
    // The rebuilding worker dies before answering (it used to leave every later call waiting).
    k.w().onerror?.({ message: "boom again", preventDefault() {} } as unknown as ErrorEvent);
    await flush();
    expect(k.made).toHaveLength(3);
    expect(k.w().last()).toMatchObject({ kind: "replay", json: '[{"op":"a"}]' });
    k.w().answer({ ok: true, scene });
    await flush();
    expect(k.restarts).toEqual([{ reason: "the kernel worker failed (boom)", scene }]);
    const v = k.c.version();
    await flush();
    k.w().answer({ ok: true, version: "2" });
    await expect(v).resolves.toBe("2");
  });

  it("a log that traps every time it is replayed gives way to a new document, and the app is told why", async () => {
    const k = client();
    await applied(k, '{"op":"poison"}');
    const p = k.c.apply({ op: "x" } as never);
    await flush();
    k.w().answer({ ok: false, error: "RuntimeError: unreachable", fatal: true });
    await expect(p).rejects.toBeInstanceOf(KernelRestarted);
    for (let i = 0; i < 2; i++) {
      await flush();
      expect(k.w().last().kind).toBe("replay");
      k.w().answer({ ok: false, error: "RuntimeError: unreachable", fatal: true });
    }
    await flush();
    expect(k.w().last().kind).toBe("init");
    k.w().answer({ ok: true, scene });
    await flush();
    expect(k.restarts).toHaveLength(1);
    expect(k.restarts[0]).toMatchObject({ scene, error: expect.stringContaining("unreachable") });
  });

  it("hands the bytes of an import to the worker without copying them", async () => {
    const k = client();
    const bytes = new Uint8Array(16);
    const posted: Transferable[][] = [];
    const w = k.w();
    w.postMessage = ((m: WorkerRequest, t?: Transferable[]) => {
      w.posted.push(m);
      posted.push(t ?? []);
    }) as FakeWorker["postMessage"];
    void k.c.importFile("a.stl", bytes).catch(() => undefined);
    await flush();
    expect(posted[0]).toEqual([bytes.buffer]);
  });
});
