// Whether the app offers renders at all (lane 15). Render credits are not launched: the
// Credits chip and the Render pane appear only when a render service is configured (in
// Settings, saved on this device; a development build also takes VITE_RENDER_SERVICE_URL)
// AND that service answers its health check when the app starts (or when the address is
// changed). Otherwise nothing about credits is shown, and one line in Settings says why.
//
// Lane 20: "answers" means the render service's own answer, not any 2xx: GET <url>/healthz
// must return JSON that names the service ({"service": "sawhorse-render", "version": ...}).
// A catch-all server (a static host, a router's page, a proxy's sign-in page) answers 200
// with HTML to every path; that keeps the chip and the Render pane hidden.
//
// The probe follows the same gate (lane 12c): with no address configured nothing is asked
// at all (a development build no longer falls back to the local service's default address,
// which logged a refused connection on every start), and one start asks once: React's
// development double mount shares the request already on its way.
import { useCallback, useEffect, useState } from "react";

/** Where the address typed in Settings is kept (this browser or this desktop app only). */
export const RENDER_URL_KEY = "sawhorse.renderServiceUrl";
/** How long the health check may take before the service counts as not answering. */
export const HEALTH_TIMEOUT_MS = 4_000;

export type GateStatus = "off" | "checking" | "ready" | "unreachable" | "not_render";

/** What the render service's health check names itself (render-service `GET /healthz`). */
export const RENDER_SERVICE_NAME = "sawhorse-render";

/** What a health check came to: the render service answered ("ok"), nothing answered in
 * time or with a 2xx ("none"), or something else answered ("other": not the service). */
export type HealthAnswer = "ok" | "none" | "other";

/** Whether a health check's body is the render service's own answer: a JSON object with
 * `service: "sawhorse-render"` and a non-empty string `version`. */
export function isRenderHealth(body: unknown): boolean {
  if (!body || typeof body !== "object" || Array.isArray(body)) return false;
  const b = body as Record<string, unknown>;
  return b.service === RENDER_SERVICE_NAME && typeof b.version === "string" && b.version.trim() !== "";
}

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

/** A development build's address from VITE_RENDER_SERVICE_URL, when one is set; none in a
 * release build, and none when the variable is not set (nothing is configured then, so
 * nothing is asked: start the local service with VITE_RENDER_SERVICE_URL, or set it in
 * Settings). */
export function devServiceUrl(): string | null {
  const e = env();
  if (!e.DEV) return null;
  const v = typeof e.VITE_RENDER_SERVICE_URL === "string" ? e.VITE_RENDER_SERVICE_URL.trim() : "";
  return v ? v.replace(/\/+$/, "") : null;
}

/** The address the app uses: the one saved in Settings, else a development build's. */
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

/** Ask `GET /healthz` (lane 20): "ok" only for the render service's own JSON answer within
 * the timeout; "none" for no answer, an error or a status other than 2xx; "other" for a 2xx
 * that is not the service (HTML, other JSON, a body too large to be a health check). */
export async function checkService(url: string, fetchImpl: typeof fetch = (...a) => fetch(...a), timeoutMs = HEALTH_TIMEOUT_MS): Promise<HealthAnswer> {
  const ctl = typeof AbortController !== "undefined" ? new AbortController() : null;
  const timer = setTimeout(() => ctl?.abort(), timeoutMs);
  try {
    const r = await fetchImpl(`${url}/healthz`, { method: "GET", signal: ctl?.signal, cache: "no-store", headers: { Accept: "application/json" } });
    if (!r.ok) return "none";
    const text = await r.text();
    // The service's answer is a few hundred bytes; a page is not a health check.
    if (text.length > 64 * 1024) return "other";
    let body: unknown;
    try {
      body = JSON.parse(text);
    } catch {
      return "other";
    }
    return isRenderHealth(body) ? "ok" : "other";
  } catch {
    return "none";
  } finally {
    clearTimeout(timer);
  }
}

/** The render service answers `GET /healthz` as itself within the timeout (lane 20: its own
 * JSON, not any 2xx). */
export async function serviceHealthy(url: string, fetchImpl: typeof fetch = (...a) => fetch(...a), timeoutMs = HEALTH_TIMEOUT_MS): Promise<boolean> {
  return (await checkService(url, fetchImpl, timeoutMs)) === "ok";
}

/** Requests on their way, per fetch and per `url` and round of asking: a second ask for the
 * same round (React's development double mount) shares the first instead of asking again.
 * Forgotten once answered, so a later start or a Recheck asks afresh. */
const inFlight = new WeakMap<object, Map<string, Promise<HealthAnswer>>>();
const DEFAULT_FETCH = {};

/** {@link checkService}, asked once per `url` and `round` while the answer is on its way. */
export function probeOnce(url: string, round: number, fetchImpl?: typeof fetch): Promise<HealthAnswer> {
  const owner: object = fetchImpl ?? DEFAULT_FETCH;
  let byKey = inFlight.get(owner);
  if (!byKey) inFlight.set(owner, (byKey = new Map()));
  const key = `${round}
${url}`;
  const going = byKey.get(key);
  if (going) return going;
  const p = checkService(url, fetchImpl).finally(() => byKey.delete(key));
  byKey.set(key, p);
  return p;
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
    case "not_render":
      return `Renders are off: ${url} answered, but not as a Sawhorse render service (its /healthz did not name the service). Check the address.`;
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
    void probeOnce(url, asked, fetchImpl).then((a) => {
      if (alive) setStatus(a === "ok" ? "ready" : a === "other" ? "not_render" : "unreachable");
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
