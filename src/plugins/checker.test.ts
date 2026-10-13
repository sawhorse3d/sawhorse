// Lane 16 (finding 13): a large plugin package is checked in a worker, a small one here; both
// give what loadPackage gives, and a check that runs away is refused and takes its worker.
import { afterEach, describe, expect, it, vi } from "vitest";
import { checkPackage } from "../../packages/plugin-api/src/source.ts";
import { loadPackage } from "./loader";
import { packageDigest } from "./store";

function manifest() {
  return JSON.stringify({ id: "org.example.big", name: "Big", version: "1.0.0", api: 1, description: "", author: "Sawhorse", license: "MIT", permissions: [] });
}

const big = () => {
  const lines = ['import { ui } from "sawhorse";'];
  for (let i = 0; i < 6000; i++) lines.push(`export function f${i}(a) { return a + ${i}; }`);
  return { "plugin.json": manifest(), "main.js": lines.join("\n") };
};

type Posted = { id: number; files: Record<string, string>; entry: string | null };

/** A stand-in for check.worker.ts: answers like it, or never (`hang`). */
function fakeWorker(mode: "answer" | "hang" | "crash") {
  const made: FakeWorker[] = [];
  class FakeWorker {
    onmessage: ((e: MessageEvent) => void) | null = null;
    onerror: ((e: ErrorEvent) => void) | null = null;
    posted: Posted[] = [];
    terminated = false;
    constructor() {
      made.push(this);
    }
    postMessage(m: Posted) {
      this.posted.push(m);
      if (mode === "hang") return;
      setTimeout(() => {
        if (mode === "crash") return this.onerror?.({ message: "boom" } as ErrorEvent);
        const check = m.entry === null ? null : checkPackage(m.files, m.entry);
        this.onmessage?.({ data: { id: m.id, ok: true, check, digest: packageDigest(m.files) } } as MessageEvent);
      }, 0);
    }
    terminate() {
      this.terminated = true;
    }
  }
  return { FakeWorker, made };
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
  vi.resetModules();
});

describe("preparePackage", () => {
  it("checks a small package here, the same as loadPackage", async () => {
    const { FakeWorker, made } = fakeWorker("answer");
    vi.stubGlobal("Worker", FakeWorker);
    const { preparePackage } = await import("./checker");
    const files = { "plugin.json": manifest(), "main.js": 'import { ui } from "sawhorse";\nexport {};' };
    const p = await preparePackage(files, "session");
    expect(made).toHaveLength(0);
    expect(p.result.ok).toBe(true);
    expect(p.digest).toBe(packageDigest(files));
  });

  it("checks a large package in the worker and gets the same package and digest", async () => {
    const { FakeWorker, made } = fakeWorker("answer");
    vi.stubGlobal("Worker", FakeWorker);
    const { preparePackage, CHECK_IN_WORKER } = await import("./checker");
    const files = big();
    expect(files["main.js"].length).toBeGreaterThan(CHECK_IN_WORKER);
    const p = await preparePackage(files, "unpacked", "/plugins/big");
    expect(made).toHaveLength(1);
    expect(made[0].posted[0].entry).toBe("main.js");
    const direct = loadPackage(files, "unpacked", "/plugins/big");
    expect(p.result.ok && direct.ok).toBe(true);
    if (p.result.ok && direct.ok) expect(p.result.pkg.modules).toEqual(direct.pkg.modules);
    expect(p.digest).toBe(packageDigest(files));
  });

  it("refuses what the worker refuses: a remote import in a large package", async () => {
    const { FakeWorker } = fakeWorker("answer");
    vi.stubGlobal("Worker", FakeWorker);
    const { preparePackage } = await import("./checker");
    const files = big();
    files["main.js"] = 'import x from "https://cdn.example.com/x.js";\n' + files["main.js"];
    const p = await preparePackage(files, "session");
    expect(p.result.ok).toBe(false);
    expect(p.digest).toBe("");
  });

  it("refuses a check that runs past its time and replaces the worker", async () => {
    vi.useFakeTimers();
    const { FakeWorker, made } = fakeWorker("hang");
    vi.stubGlobal("Worker", FakeWorker);
    const { preparePackage, CHECK_SECONDS } = await import("./checker");
    const pending = preparePackage(big(), "session");
    const caught = pending.catch((e: Error) => e);
    await vi.advanceTimersByTimeAsync(CHECK_SECONDS * 1000 + 1);
    const err = await caught;
    expect(err).toBeInstanceOf(Error);
    expect((err as Error).message).toContain(`more than ${CHECK_SECONDS} s`);
    expect(made[0].terminated).toBe(true);
    // The next large check starts a new worker.
    void preparePackage(big(), "session").catch(() => undefined);
    expect(made).toHaveLength(2);
  });

  it("refuses the waiting checks when the checker itself fails", async () => {
    const { FakeWorker } = fakeWorker("crash");
    vi.stubGlobal("Worker", FakeWorker);
    const { preparePackage } = await import("./checker");
    await expect(preparePackage(big(), "session")).rejects.toThrow("could not be checked");
  });
});
