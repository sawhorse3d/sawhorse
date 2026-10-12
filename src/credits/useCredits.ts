// The credits state the top bar's chip and the Render pane share: connection to the render
// service, this device's balance, and watching for a checkout to land.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { RenderServiceClient, ServiceError, ServiceUnreachableError, serviceUrl, type Balance } from "./client";
import { deviceKey, DeviceKeyUnavailableError, type KeyStorage } from "./device";

export type Connection = "connecting" | "connected" | "unreachable" | "unlinked" | "unsupported" | "error";

export interface CreditsState {
  connection: Connection;
  /** Credits on this device's balance; null until the service has answered. */
  credits: number | null;
  balance: Balance | null;
  installId: string | null;
  storage: KeyStorage | null;
  /** Why the service is not usable right now (shown in the Render pane). */
  problem: string | null;
  /** A checkout was opened; the balance is polled until it grows. */
  awaitingPayment: boolean;
}

export interface CreditsHandle {
  state: CreditsState;
  client: RenderServiceClient;
  refresh: () => Promise<Balance | null>;
  /** After a render: the service reports the new balance. */
  setCredits: (n: number) => void;
  /** Poll for a pack bought in the system browser. */
  watchCheckout: () => void;
}

let factory: () => RenderServiceClient = () => new RenderServiceClient(serviceUrl(), deviceKey);

/** Tests replace the client every useCredits() without an explicit one gets. */
export function setDefaultCreditsClient(f: () => RenderServiceClient): void {
  factory = f;
}

/** Retries while the service cannot be reached back off: 10 s, 30 s, 60 s, then every 5 min
 * (a dev build without the service running asks a dozen times an hour, not 360). */
export const RETRY_BACKOFF_MS = [10_000, 30_000, 60_000, 5 * 60_000] as const;
/** The first retry (also how soon a window focus may ask again while unreachable). */
export const RETRY_UNREACHABLE_MS = RETRY_BACKOFF_MS[0];
export const CHECKOUT_POLL_MS = 3_000;

/** How long to wait after `failures` unreachable answers in a row (1 or more). */
export function retryDelay(failures: number): number {
  return RETRY_BACKOFF_MS[Math.min(Math.max(failures, 1), RETRY_BACKOFF_MS.length) - 1];
}

/** Quiet by design: an absent render service is normal (offline, or a dev build without it),
 * so it is a debug line, never a console error. Browsers still print their own line for
 * each failed request; the back-off keeps those rare. */
let debug = (message: string): void => {
  const env = (import.meta as unknown as { env?: { MODE?: string } }).env;
  if (env?.MODE !== "test" && typeof console !== "undefined") console.debug(`[credits] ${message}`);
};

/** Tests watch the debug lines (unit tests are quiet by default). */
export function setCreditsLogger(f: (message: string) => void): void {
  debug = f;
}
export const CHECKOUT_WATCH_MS = 10 * 60_000;

export function useCredits(explicit?: RenderServiceClient): CreditsHandle {
  const client = useMemo(() => explicit ?? factory(), [explicit]);
  const [state, setState] = useState<CreditsState>({
    connection: "connecting",
    credits: null,
    balance: null,
    installId: null,
    storage: null,
    problem: null,
    awaitingPayment: false,
  });
  const watch = useRef<{ until: number; baseline: number } | null>(null);
  const alive = useRef(true);
  /** Unreachable answers in a row (the back-off step), and when the service was last asked. */
  const failures = useRef(0);
  const lastAsked = useRef(0);
  /** Bumped after every unreachable answer, so the retry timer is set again. */
  const [retries, setRetries] = useState(0);

  const refresh = useCallback(async (): Promise<Balance | null> => {
    lastAsked.current = Date.now();
    try {
      const b = await client.balance();
      if (failures.current) debug(`render service reachable again (${client.baseUrl}).`);
      failures.current = 0;
      if (!alive.current) return b;
      const w = watch.current;
      const landed = w ? b.credits > w.baseline : false;
      if (landed || (w && Date.now() > w.until)) watch.current = null;
      setState((s) => ({ ...s, connection: "connected", credits: b.credits, balance: b, installId: b.install_id, problem: null, awaitingPayment: !!watch.current }));
      return b;
    } catch (err) {
      if (!alive.current) return null;
      let connection: Connection = "error";
      let problem = err instanceof Error ? err.message : String(err);
      if (err instanceof ServiceUnreachableError) {
        connection = "unreachable";
        problem = `Render service not connected (${client.baseUrl}).`;
        failures.current++;
        debug(`render service not reachable (${client.baseUrl}); asking again in ${retryDelay(failures.current) / 1000} s.`);
        setRetries((n) => n + 1);
      } else if (err instanceof ServiceError && err.code === "device_unlinked") connection = "unlinked";
      else if (err instanceof DeviceKeyUnavailableError) connection = "unsupported";
      setState((s) => ({ ...s, connection, credits: connection === "unreachable" ? s.credits : null, problem }));
      return null;
    }
  }, [client]);

  useEffect(() => {
    alive.current = true;
    deviceKey()
      .then((k) => alive.current && setState((s) => ({ ...s, storage: k.storage, installId: s.installId ?? k.installId })))
      .catch(() => {});
    void refresh();
    // Back in the window: ask now, unless the service was unreachable a moment ago (the
    // back-off timer is on it).
    const onFocus = () => {
      if (failures.current && Date.now() - lastAsked.current < RETRY_UNREACHABLE_MS) return;
      void refresh();
    };
    window.addEventListener("focus", onFocus);
    return () => {
      alive.current = false;
      window.removeEventListener("focus", onFocus);
    };
  }, [refresh]);

  // Poll while a checkout is open; retry, backing off, while the service is down.
  const { connection, awaitingPayment } = state;
  useEffect(() => {
    if (!awaitingPayment) return;
    const t = setInterval(() => void refresh(), CHECKOUT_POLL_MS);
    return () => clearInterval(t);
  }, [awaitingPayment, refresh]);
  useEffect(() => {
    if (awaitingPayment || connection !== "unreachable") return;
    const t = setTimeout(() => void refresh(), retryDelay(failures.current));
    return () => clearTimeout(t);
  }, [connection, awaitingPayment, refresh, retries]);

  const setCredits = useCallback((n: number) => setState((s) => ({ ...s, credits: n, balance: s.balance ? { ...s.balance, credits: n } : s.balance })), []);

  const watchCheckout = useCallback(() => {
    watch.current = { until: Date.now() + CHECKOUT_WATCH_MS, baseline: state.credits ?? 0 };
    setState((s) => ({ ...s, awaitingPayment: true }));
  }, [state.credits]);

  return { state, client, refresh, setCredits, watchCheckout };
}
