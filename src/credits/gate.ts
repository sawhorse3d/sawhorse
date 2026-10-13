// Whether the app offers renders at all (lane 15). Render credits are not launched: the
// Credits chip and the Render pane appear only when a render service is configured (in
// Settings, saved on this device; a development build also takes VITE_RENDER_SERVICE_URL)
// AND that service answers its health check when the app starts (or when the address is
// changed). Otherwise nothing about credits is shown, and one line in Settings says why.
import { useCallback, useEffect, useState } from "react";
import { DEFAULT_SERVICE_URL } from "./client";

/** Where the address typed in Settings is kept (this browser or this desktop app only). */
export const RENDER_URL_KEY = "sawhorse.renderServiceUrl";
/** How long the health check may take before the service counts as not answering. */
export const HEALTH_TIMEOUT_MS = 4_000;

export type GateStatus = "off" | "checking" | "ready" | "unreachable";

function env(): Record<string, unknown> {
  return (import.meta as unknown as { env?: Record<string, unknown> }).env ?? {};
}

/** The address saved in Settings, if any. */
export function storedServiceUrl(): string | null {
  try {
    const v = localStorage.getItem(RENDER_URL_KEY);
    return v && v.trim() ? v.trim().replace(/\/+$/, "") : null;
  } catch {
    return null;
  }
}

/** Save (or with null / "" forget) the address typed in Settings. */
export function saveServiceUrl(url: string | null): void {
  try {
    const v = (url ?? "").trim();
    if (v) localStorage.setItem(RENDER_URL_KEY, v.replace(/\/+$/, ""));
    else localStorage.removeItem(RENDER_URL_KEY);
  } catch {
    /* storage blocked: the address lasts for this session only (the caller keeps it) */
  }
}

/** A development build's default (the local render service); none in a release build. */
export function devServiceUrl(): string | null {
  const e = env();
  if (!e.DEV) return null;
  const v = typeof e.VITE_RENDER_SERVICE_URL === "string" && e.VITE_RENDER_SERVICE_URL ? e.VITE_RENDER_SERVICE_URL : DEFAULT_SERVICE_URL;
  return v.replace(/\/+$/, "");
}

/** The address the app uses: the one saved in Settings, else a development default. */
export function configuredServiceUrl(): string | null {
  return storedServiceUrl() ?? devServiceUrl();
}

/** A plain http(s) address, or the reason it is not one. */
export function checkServiceUrl(text: string): { ok: true; url: string } | { ok: false; error: string } {
  const t = text.trim();
  if (!t) return { ok: true, url: "" };
  let u: URL;
  try {
    u = new URL(t);
  } catch {
    return { ok: false, error: `"${t}" is not a web address (it starts with https://).` };
  }
  if (u.protocol !== "https:" && u.protocol !== "http:") return { ok: false, error: "Use an https:// address (http:// only for a service on this computer)." };
  if (u.protocol === "http:" && !["localhost", "127.0.0.1", "[::1]"].includes(u.hostname)) return { ok: false, error: "Use https:// for a service on another computer." };
  return { ok: true, url: `${u.origin}${u.pathname}`.replace(/\/+$/, "") };
}

/** The service answers `GET /healthz` with a 2xx within the timeout. */
export async function serviceHealthy(url: string, fetchImpl: typeof fetch = (...a) => fetch(...a), timeoutMs = HEALTH_TIMEOUT_MS): Promise<boolean> {
  const ctl = typeof AbortController !== "undefined" ? new AbortController() : null;
  const timer = setTimeout(() => ctl?.abort(), timeoutMs);
  try {
    const r = await fetchImpl(`${url}/healthz`, { method: "GET", signal: ctl?.signal, cache: "no-store" });
    return r.ok;
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}

/** What Settings says about renders, in one line. */
export function gateLine(status: GateStatus, url: string | null): string {
  switch (status) {
    case "off":
      return "Renders are off: no render service is set. Sawhorse needs none to model, list or nest.";
    case "checking":
      return `Asking the render service at ${url} if it is there…`;
    case "unreachable":
      return `Renders are off: the render service at ${url} did not answer its health check.`;
    case "ready":
      return `Renders are on: the render service at ${url} answered. The Credits chip and the Render pane are shown.`;
  }
}

export interface RenderGate {
  url: string | null;
  status: GateStatus;
  /** Save a new address (null or "": none) and check it at once. */
  setUrl: (url: string | null) => void;
  /** Ask the service again. */
  recheck: () => void;
}

export function useRenderGate(fetchImpl?: typeof fetch): RenderGate {
  const [url, setUrlState] = useState<string | null>(configuredServiceUrl);
  const [status, setStatus] = useState<GateStatus>(() => (configuredServiceUrl() ? "checking" : "off"));
  const [asked, setAsked] = useState(0);
  useEffect(() => {
    if (!url) {
      setStatus("off");
      return;
    }
    let alive = true;
    setStatus("checking");
    void serviceHealthy(url, fetchImpl).then((ok) => {
      if (alive) setStatus(ok ? "ready" : "unreachable");
    });
    return () => {
      alive = false;
    };
  }, [url, asked, fetchImpl]);
  const setUrl = useCallback((next: string | null) => {
    saveServiceUrl(next);
    // Storage may be blocked: the address still holds for this session.
    setUrlState(storedServiceUrl() ?? ((next ?? "").trim() ? (next ?? "").trim().replace(/\/+$/, "") : devServiceUrl()));
    setAsked((n) => n + 1);
  }, []);
  const recheck = useCallback(() => setAsked((n) => n + 1), []);
  return { url, status, setUrl, recheck };
}
