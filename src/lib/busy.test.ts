import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook } from "@testing-library/react";
import { BUSY_AFTER_MS, busySeconds, useBusySeconds } from "./busy";

// #144: nothing on screen said a long edit was running. The status line and the cursor say
// so once a request to the kernel has waited about a second for its answer.

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
  type P = { since: number | null };
  const hook = (since: number | null, values: (number | null)[] = []) =>
    renderHook(({ since }: P) => {
      const v = useBusySeconds(since);
      values.push(v);
      return v;
    }, { initialProps: { since } as P });

  it("shows the count as soon as it is told (the client tells it a second in), and counts up", () => {
    vi.useFakeTimers({ now: 10_000 });
    const { result, rerender } = hook(null);
    expect(result.current).toBeNull();
    rerender({ since: 9_000 });
    expect(result.current).toBe(1);
    act(() => void vi.advanceTimersByTime(3_000));
    expect(result.current).toBe(4);
    rerender({ since: null });
    expect(result.current).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("a new wait never shows the last one's count, not for a single draw", () => {
    vi.useFakeTimers({ now: 0 });
    const values: (number | null)[] = [];
    const { result, rerender } = hook(null, values);
    act(() => void vi.advanceTimersByTime(4_000));
    rerender({ since: 0 });
    expect(result.current).toBe(4);
    const from = values.length;
    act(() => void vi.advanceTimersByTime(1_000));
    rerender({ since: 4_000 });
    expect(result.current).toBe(1);
    expect(values.slice(from).filter((v) => v !== null && v > 1)).toEqual([5]);
    expect(values.at(-1)).toBe(1);
  });

  it("draws again once a second while one waits, not at every reading", () => {
    vi.useFakeTimers({ now: 1_000 });
    let renders = 0;
    const { rerender } = renderHook(({ since }: P) => {
      renders++;
      return useBusySeconds(since);
    }, { initialProps: { since: null } as P });
    rerender({ since: 0 });
    const before = renders;
    // Twenty readings, each its own turn of the clock (one act would draw them as one).
    for (let i = 0; i < 20; i++) act(() => void vi.advanceTimersByTime(250));
    // 1 s shown, then 2, 3, 4, 5, 6. React may draw once more before it sees a reading
    // changed nothing, so up to twice that; a draw at every reading is twenty.
    expect(renders - before).toBeLessThanOrEqual(12);
  });
});
