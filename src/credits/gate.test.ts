// The render gate (lane 15): renders appear only with a service set and answering.
import { afterEach, describe, expect, it } from "vitest";
import { checkServiceUrl, configuredServiceUrl, devServiceUrl, gateLine, RENDER_URL_KEY, saveServiceUrl, serviceHealthy, storedServiceUrl } from "./gate";

afterEach(() => localStorage.removeItem(RENDER_URL_KEY));

describe("render gate", () => {
  it("keeps the address typed in Settings, trimmed, and forgets it on clear", () => {
    expect(storedServiceUrl()).toBeNull();
    saveServiceUrl("  https://render.example.com/  ");
    expect(storedServiceUrl()).toBe("https://render.example.com");
    expect(configuredServiceUrl()).toBe("https://render.example.com");
    saveServiceUrl("");
    expect(storedServiceUrl()).toBeNull();
    // A development build (vitest runs as one) falls back to the local service.
    expect(devServiceUrl()).toBe("http://127.0.0.1:8787");
    expect(configuredServiceUrl()).toBe("http://127.0.0.1:8787");
  });

  it("takes https anywhere, http only on this computer", () => {
    expect(checkServiceUrl("https://render.example.com/v1/")).toEqual({ ok: true, url: "https://render.example.com/v1" });
    expect(checkServiceUrl("http://127.0.0.1:8787")).toEqual({ ok: true, url: "http://127.0.0.1:8787" });
    expect(checkServiceUrl("http://localhost:9000/")).toEqual({ ok: true, url: "http://localhost:9000" });
    expect(checkServiceUrl("")).toEqual({ ok: true, url: "" });
    expect(checkServiceUrl("http://render.example.com")).toMatchObject({ ok: false, error: expect.stringContaining("https://") });
    expect(checkServiceUrl("ftp://x")).toMatchObject({ ok: false });
    expect(checkServiceUrl("render dot com")).toMatchObject({ ok: false, error: expect.stringContaining("not a web address") });
  });

  it("asks GET /healthz: a 2xx is up; an error, a 5xx or no answer in time is down", async () => {
    const seen: string[] = [];
    const ok = (status: number) => (async (u: RequestInfo | URL) => (seen.push(String(u)), new Response("{}", { status }))) as typeof fetch;
    expect(await serviceHealthy("https://svc.test", ok(200))).toBe(true);
    expect(seen).toEqual(["https://svc.test/healthz"]);
    expect(await serviceHealthy("https://svc.test", ok(503))).toBe(false);
    expect(await serviceHealthy("https://svc.test", (async () => Promise.reject(new TypeError("offline"))) as typeof fetch)).toBe(false);
    const hang = ((_u: RequestInfo | URL, init?: RequestInit) =>
      new Promise<Response>((_, reject) => init?.signal?.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError"))))) as typeof fetch;
    expect(await serviceHealthy("https://svc.test", hang, 30)).toBe(false);
  });

  it("says in one line why renders are off or on", () => {
    expect(gateLine("off", null)).toBe("Renders are off: no render service is set. Sawhorse needs none to model, list or nest.");
    expect(gateLine("unreachable", "https://svc.test")).toBe("Renders are off: the render service at https://svc.test did not answer its health check.");
    expect(gateLine("ready", "https://svc.test")).toMatch(/^Renders are on/);
    expect(gateLine("checking", "https://svc.test")).toMatch(/^Asking/);
  });
});
