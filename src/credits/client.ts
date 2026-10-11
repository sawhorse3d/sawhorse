// Client for the Sawhorse render service (render-service/, ADR-0004). Every call is signed
// with the device key (see ./device.ts). Provider keys never reach this app.
import { b64url, canonicalString, type DeviceKey } from "./device";

/** Dev default; a build points elsewhere with VITE_RENDER_SERVICE_URL. */
export const DEFAULT_SERVICE_URL = "http://127.0.0.1:8787";

export function serviceUrl(): string {
  const env = (import.meta as unknown as { env?: Record<string, string | undefined> }).env;
  return (env?.VITE_RENDER_SERVICE_URL || DEFAULT_SERVICE_URL).replace(/\/+$/, "");
}

export interface Pack {
  id: string;
  credits: number;
  amount: number;
  currency: string;
  label?: string;
}

export const PACKS: Pack[] = [
  { id: "p100", credits: 100, amount: 1000, currency: "usd", label: "$10" },
  { id: "p250", credits: 250, amount: 2500, currency: "usd", label: "$25" },
  { id: "p500", credits: 500, amount: 5000, currency: "usd", label: "$50" },
];

export interface Balance {
  install_id: string;
  credits: number;
  devices: number;
  max_devices: number;
  render_cost: number;
  packs: Pack[];
}

export interface RenderRequest {
  request_id: string;
  prompt: string;
  preset?: string | null;
  depth_png: string;
  edges_png?: string;
  clay_png?: string;
  return?: "url" | "inline";
}

export interface RenderResult {
  request_id: string;
  status: "succeeded";
  provider: string;
  mock: boolean;
  width: number | null;
  height: number | null;
  image_url: string;
  expires_at: string;
  balance: number;
}

export interface HistoryItem {
  request_id: string;
  status: "pending" | "succeeded" | "refunded";
  prompt: string;
  preset: string | null;
  provider: string | null;
  created_at: string;
  image_url?: string;
}

/** The service could not be reached at all (not running, offline, blocked). */
export class ServiceUnreachableError extends Error {}

/** The service answered with an error. `code` is its stable error code. */
export class ServiceError extends Error {
  readonly status: number;
  readonly code: string;
  readonly body: Record<string, unknown>;
  constructor(status: number, code: string, message: string, body: Record<string, unknown>) {
    super(message);
    this.status = status;
    this.code = code;
    this.body = body;
  }
}

export type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;

function nonce(): string {
  return b64url(crypto.getRandomValues(new Uint8Array(16)));
}

export class RenderServiceClient {
  readonly baseUrl: string;
  private readonly key: () => Promise<DeviceKey>;
  private readonly fetchImpl: FetchLike;
  /** Server clock minus ours, seconds; learned from a skew refusal's Date header. */
  private clockOffset = 0;

  constructor(baseUrl: string, key: () => Promise<DeviceKey>, fetchImpl: FetchLike = (i, init) => fetch(i, init)) {
    this.baseUrl = baseUrl.replace(/\/+$/, "");
    this.key = key;
    this.fetchImpl = fetchImpl;
  }

  /** Signed request; retries once after correcting for a skewed local clock. */
  async signed<T>(method: "GET" | "POST", path: string, body?: unknown, timeoutMs = 15_000): Promise<T> {
    const first = await this.send(method, path, body, timeoutMs);
    if (first.status === 401) {
      const date = first.headers.get("date");
      const text = await first.clone().text();
      if (date && /skew/i.test(text)) {
        const server = Date.parse(date);
        if (Number.isFinite(server)) {
          this.clockOffset = Math.round((server - Date.now()) / 1000);
          return this.parse<T>(await this.send(method, path, body, timeoutMs));
        }
      }
    }
    return this.parse<T>(first);
  }

  private async send(method: string, path: string, body: unknown, timeoutMs: number): Promise<Response> {
    const device = await this.key();
    const text = body === undefined ? "" : JSON.stringify(body);
    const bytes = new TextEncoder().encode(text);
    const ts = String(Math.floor(Date.now() / 1000) + this.clockOffset);
    const n = nonce();
    const message = await canonicalString(method, path, bytes, ts, n);
    const signature = await device.sign(new TextEncoder().encode(message));
    const headers: Record<string, string> = {
      "X-Install-Id": device.installId,
      "X-Public-Key": b64url(device.publicKey),
      "X-Timestamp": ts,
      "X-Nonce": n,
      "X-Signature": b64url(signature),
    };
    if (body !== undefined) headers["Content-Type"] = "application/json";
    const ctrl = new AbortController();
    const timer = setTimeout(() => ctrl.abort(), timeoutMs);
    try {
      return await this.fetchImpl(`${this.baseUrl}${path}`, { method, headers, body: body === undefined ? undefined : bytes, signal: ctrl.signal });
    } catch (err) {
      throw new ServiceUnreachableError(ctrl.signal.aborted ? `The render service did not answer within ${Math.round(timeoutMs / 1000)} s.` : `Render service not connected (${this.baseUrl}).`, { cause: err });
    } finally {
      clearTimeout(timer);
    }
  }

  private async parse<T>(r: Response): Promise<T> {
    let json: Record<string, unknown> = {};
    try {
      json = (await r.json()) as Record<string, unknown>;
    } catch {
      /* not JSON */
    }
    if (!r.ok) {
      const retry = r.headers.get("retry-after");
      throw new ServiceError(r.status, String(json.error ?? `http_${r.status}`), String(json.message ?? `The render service answered HTTP ${r.status}.`), { ...json, ...(retry ? { retry_after: Number(retry) } : {}) });
    }
    return json as T;
  }

  async health(): Promise<{ ok: boolean; stripe: string; providers: string[]; checkout: string }> {
    try {
      const r = await this.fetchImpl(`${this.baseUrl}/healthz`);
      return (await r.json()) as { ok: boolean; stripe: string; providers: string[]; checkout: string };
    } catch (err) {
      throw new ServiceUnreachableError(`Render service not connected (${this.baseUrl}).`, { cause: err });
    }
  }

  balance(): Promise<Balance> {
    return this.signed<Balance>("GET", "/v1/balance");
  }

  checkout(pack: string): Promise<{ session_id: string; url: string; mode: string; credits: number }> {
    return this.signed("POST", "/v1/checkout", { pack });
  }

  render(req: RenderRequest): Promise<RenderResult> {
    // Providers can take a while; the service retries and fails over inside this one call.
    return this.signed<RenderResult>("POST", "/v1/render", req, 5 * 60_000);
  }

  history(limit = 24): Promise<{ renders: HistoryItem[] }> {
    return this.signed("GET", `/v1/renders?limit=${limit}`);
  }

  claimRequest(email: string): Promise<{ sent: boolean; message: string }> {
    return this.signed("POST", "/v1/claim", { step: "request", email });
  }

  claimRedeem(email: string, code: string): Promise<{ ok: boolean; credits: number; merged: number; unlinked_devices: number; devices: number }> {
    return this.signed("POST", "/v1/claim", { step: "redeem", email, code });
  }
}

export function newRequestId(): string {
  return b64url(crypto.getRandomValues(new Uint8Array(16)));
}

/** Open a URL (Stripe Checkout) in the system browser, never inside the app's webview. */
export async function openExternal(url: string): Promise<void> {
  const t = (globalThis as unknown as { __TAURI_INTERNALS__?: { invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> } }).__TAURI_INTERNALS__;
  if (t) {
    await t.invoke("open_external", { url });
    return;
  }
  const w = window.open(url, "_blank", "noopener,noreferrer");
  // noopener makes window.open return null even on success, so only a thrown error is a failure.
  void w;
}
