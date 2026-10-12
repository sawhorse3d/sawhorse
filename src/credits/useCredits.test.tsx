// The render service's balance poll when the service is down (lane 7b): it backs off (10 s,
// 30 s, 60 s, then every 5 min), logs at debug level only, and comes back on its own.
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RenderServiceClient, ServiceUnreachableError, type Balance } from "./client";
import { RETRY_BACKOFF_MS, retryDelay, setCreditsLogger, useCredits } from "./useCredits";

const balance: Balance = { install_id: "i", credits: 7, devices: 1, max_devices: 3, render_cost: 1, packs: [] };

function flaky() {
  let up = false;
  const calls: number[] = [];
  const client = {
    baseUrl: "http://render.test",
    balance: vi.fn(async () => {
      calls.push(Date.now());
      if (!up) throw new ServiceUnreachableError("Render service not connected (http://render.test).");
      return balance;
    }),
  } as unknown as RenderServiceClient;
  return { client, calls, setUp: (v: boolean) => (up = v) };
}

const flush = () => act(async () => {});

describe("the balance poll while the render service is unreachable", () => {
  const lines: string[] = [];
  beforeEach(() => {
    vi.useFakeTimers({ now: 0 });
    lines.length = 0;
    setCreditsLogger((m) => lines.push(m));
  });
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("the back-off steps", () => {
    expect(RETRY_BACKOFF_MS).toEqual([10_000, 30_000, 60_000, 300_000]);
    expect([0, 1, 2, 3, 4, 50].map(retryDelay)).toEqual([10_000, 10_000, 30_000, 60_000, 300_000, 300_000]);
  });

  it("asks at 0, 10 s, 40 s, 100 s, then every 5 min; debug lines only; reconnects", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const f = flaky();
    const { result } = renderHook(() => useCredits(f.client));
    await flush();
    expect(result.current.state.connection).toBe("unreachable");
    const at = async (ms: number) => {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(ms - Date.now());
      });
    };
    await at(9_999);
    expect(f.calls).toEqual([0]);
    await at(10_000);
    await at(39_999);
    expect(f.calls).toEqual([0, 10_000]);
    await at(40_000);
    await at(100_000);
    await at(400_000);
    await at(700_000);
    expect(f.calls).toEqual([0, 10_000, 40_000, 100_000, 400_000, 700_000]);
    // A window focus right after a failure does not ask again (the timer is on it).
    window.dispatchEvent(new Event("focus"));
    await flush();
    expect(f.calls).toHaveLength(6);
    expect(lines).toHaveLength(6);
    expect(lines[0]).toBe("render service not reachable (http://render.test); asking again in 10 s.");
    expect(lines[3]).toContain("asking again in 300 s");
    // The service comes up: the next try connects and the poll stops.
    f.setUp(true);
    await at(1_000_000);
    expect(result.current.state.connection).toBe("connected");
    expect(result.current.state.credits).toBe(7);
    expect(lines.at(-1)).toBe("render service reachable again (http://render.test).");
    await at(3_000_000);
    expect(f.calls).toHaveLength(7);
    expect(error).not.toHaveBeenCalled();
    expect(warn).not.toHaveBeenCalled();
    error.mockRestore();
    warn.mockRestore();
  });

  it("a focus after the first step has passed asks at once", async () => {
    const f = flaky();
    renderHook(() => useCredits(f.client));
    await flush();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });
    expect(f.calls).toEqual([0, 10_000]);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(15_000);
    });
    window.dispatchEvent(new Event("focus"));
    await flush();
    expect(f.calls).toEqual([0, 10_000, 25_000]);
  });
});
