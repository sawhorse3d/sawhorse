// #144: while a long edit runs, nothing on screen said so. The kernel runs in a worker, so the
// window keeps drawing and looked idle, and a person could take the edit for one not taken
// and do it again. Once the kernel has worked on what it was last asked for about a second,
// the status line says "Working… 4 s" and the cursor over the model shows busy. Nothing is
// blocked: only what is shown changes.
import { useEffect, useState } from "react";

/** How long a request runs before the app says something is running (ms). */
export const BUSY_AFTER_MS = 1_000;

/** Whole seconds shown for a request sent at `since` (Date.now()), at `now`: null before
 * BUSY_AFTER_MS, when none is waiting, or when the clock went back. */
export function busySeconds(since: number | null, now: number): number | null {
  if (since === null || now - since < BUSY_AFTER_MS) return null;
  return Math.floor((now - since) / 1_000);
}

/** The seconds to show for the request waiting since `since` (KernelClient.busySince), read
 * four times a second while one waits; no timer runs while none does. It holds the number
 * shown, not the clock, so the app draws again once a second, not at every reading. */
export function useBusySeconds(since: number | null): number | null {
  const [shown, setShown] = useState<{ since: number; secs: number | null } | null>(null);
  useEffect(() => {
    if (since === null) return;
    const read = () => {
      const secs = busySeconds(since, Date.now());
      setShown((s) => (s && s.since === since && s.secs === secs ? s : { since, secs }));
    };
    read();
    const t = setInterval(read, 250);
    return () => clearInterval(t);
  }, [since]);
  return since !== null && shown?.since === since ? shown.secs : null;
}
