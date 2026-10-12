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

export const RETRY_UNREACHABLE_MS = 10_000;
export const CHECKOUT_POLL_MS = 3_000;
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

  const refresh = useCallback(async (): Promise<Balance | null> => {
    try {
      const b = await client.balance();
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
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => {
      alive.current = false;
      window.removeEventListener("focus", onFocus);
    };
  }, [refresh]);

  // Retry while the service is down; poll while a checkout is open.
  const { connection, awaitingPayment } = state;
  useEffect(() => {
    const ms = awaitingPayment ? CHECKOUT_POLL_MS : connection === "unreachable" ? RETRY_UNREACHABLE_MS : 0;
    if (!ms) return;
    const t = setInterval(() => void refresh(), ms);
    return () => clearInterval(t);
  }, [connection, awaitingPayment, refresh]);

  const setCredits = useCallback((n: number) => setState((s) => ({ ...s, credits: n, balance: s.balance ? { ...s.balance, credits: n } : s.balance })), []);

  const watchCheckout = useCallback(() => {
    watch.current = { until: Date.now() + CHECKOUT_WATCH_MS, baseline: state.credits ?? 0 };
    setState((s) => ({ ...s, awaitingPayment: true }));
  }, [state.credits]);

  return { state, client, refresh, setCredits, watchCheckout };
}
