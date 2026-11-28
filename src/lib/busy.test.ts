import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook } from "@testing-library/react";
import { BUSY_AFTER_MS, busySeconds, useBusySeconds } from "./busy";

// #144: nothing on screen said a long edit was running. The status line and the cursor say
// so once the kernel has worked on one request for about a second.

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("how long the kernel has been working, as shown", () => {
  it("nothing for the first second, then whole seconds", () => {
    expect(BUSY_AFTER_MS).toBe(1_000);
    expect(busySeconds(null, 5_000)).toBeNull();
    expect(busySeconds(1_000, 1_000)).toBeNull();
    expect(busySeconds(1_000, 1_999)).toBeNull();
    expect(busySeconds(1_000, 2_000)).toBe(1);
    expect(busySeconds(1_000, 5_999)).toBe(4);
    expect(busySeconds(1_000, 6_000)).toBe(5);
  });

  it("a clock that went back shows nothing rather than a negative time", () => {
    expect(busySeconds(9_000, 1_000)).toBeNull();
  });
});

describe("useBusySeconds", () => {
  it("counts up while a request waits and stops when it answers, with no timer left behind", () => {
    vi.useFakeTimers({ now: 10_000 });
    const { result, rerender } = renderHook(({ since }: { since: number | null }) => useBusySeconds(since), { initialProps: { since: null as number | null } });
    expect(result.current).toBeNull();
    rerender({ since: 10_000 });
    expect(result.current).toBeNull();
    act(() => void vi.advanceTimersByTime(999));
    expect(result.current).toBeNull();
    act(() => void vi.advanceTimersByTime(251));
    expect(result.current).toBe(1);
    act(() => void vi.advanceTimersByTime(3_000));
    expect(result.current).toBe(4);
    rerender({ since: null });
    expect(result.current).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("draws again once a second, not at every reading", () => {
    vi.useFakeTimers({ now: 0 });
    let renders = 0;
    const { rerender } = renderHook(({ since }: { since: number | null }) => {
      renders++;
      return useBusySeconds(since);
    }, { initialProps: { since: null as number | null } });
    rerender({ since: 0 });
    const before = renders;
    // Twenty readings, each its own turn of the clock (one act would draw them as one).
    for (let i = 0; i < 20; i++) act(() => void vi.advanceTimersByTime(250));
    // Shown nothing until 1 s, then 1, 2, 3, 4, 5. React may draw once more before it sees a
    // reading changed nothing, so up to twice that; a draw at every reading is twenty.
    expect(renders - before).toBeLessThanOrEqual(12);
  });

  it("a request sent later counts from when it was sent", () => {
    vi.useFakeTimers({ now: 0 });
    const { result, rerender } = renderHook(({ since }: { since: number | null }) => useBusySeconds(since), { initialProps: { since: 0 as number | null } });
    act(() => void vi.advanceTimersByTime(3_000));
    expect(result.current).toBe(3);
    rerender({ since: 3_000 });
    expect(result.current).toBeNull();
    act(() => void vi.advanceTimersByTime(1_000));
    expect(result.current).toBe(1);
  });
});
