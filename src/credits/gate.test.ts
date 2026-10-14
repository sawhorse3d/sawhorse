// The render gate (lane 15): renders appear only with a service set and answering.
import { StrictMode } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { checkService, checkServiceUrl, configuredServiceUrl, devServiceUrl, gateLine, isRenderHealth, probeOnce, RENDER_URL_KEY, saveServiceUrl, serviceHealthy, storedServiceUrl, useRenderGate } from "./gate";

/** The render service's own health answer (render-service GET /healthz). */
const HEALTH = JSON.stringify({ ok: true, service: "sawhorse-render", version: "0.1.0" });

afterEach(() => localStorage.removeItem(RENDER_URL_KEY));

describe("render gate", () => {
  it("keeps the address typed in Settings, trimmed, and forgets it on clear", () => {
    expect(storedServiceUrl()).toBeNull();
    saveServiceUrl("  https://render.example.com/  ");
    expect(storedServiceUrl()).toBe("https://render.example.com");
    expect(configuredServiceUrl()).toBe("https://render.example.com");
    saveServiceUrl("");
    expect(storedServiceUrl()).toBeNull();
    // Nothing set anywhere: nothing configured, even in a development build (vitest runs as
    // one), which no longer falls back to the local service's default address (lane 12c).
    expect(devServiceUrl()).toBeNull();
    expect(configuredServiceUrl()).toBeNull();
  });

  it("asks nothing when no service is configured, and once per start when one is (lane 12c)", async () => {
    const calls: string[] = [];
    const counting = (async (u: RequestInfo | URL) => (calls.push(String(u)), new Response(HEALTH, { status: 200 }))) as typeof fetch;
    // Nothing configured: off at once, no request, so no refused connection in the console.
    const off = renderHook(() => useRenderGate(counting), { wrapper: StrictMode });
    expect(off.result.current.status).toBe("off");
    await act(async () => {
      await new Promise((r) => setTimeout(r, 10));
    });
    expect(calls).toEqual([]);
    off.unmount();
    // Configured: React's development double mount (StrictMode) asks once, not twice.
    saveServiceUrl("https://svc.test");
    const on = renderHook(() => useRenderGate(counting), { wrapper: StrictMode });
    await waitFor(() => expect(on.result.current.status).toBe("ready"));
    expect(calls).toEqual(["https://svc.test/healthz"]);
    // Recheck asks again, once.
    act(() => on.result.current.recheck());
    await waitFor(() => expect(calls).toHaveLength(2));
    await act(async () => {
      await new Promise((r) => setTimeout(r, 10));
    });
    expect(calls).toHaveLength(2);
    on.unmount();
  });

  it("shares one request between two asks of the same round, and forgets it once answered", async () => {
    let n = 0;
    const slow = (() => (n++, new Promise<Response>((r) => setTimeout(() => r(new Response(HEALTH, { status: 200 })), 5)))) as typeof fetch;
    const [a, b] = [probeOnce("https://svc.test", 0, slow), probeOnce("https://svc.test", 0, slow)];
    expect(await a).toBe("ok");
    expect(await b).toBe("ok");
    expect(n).toBe(1);
    await probeOnce("https://svc.test", 0, slow);
    expect(n).toBe(2);
    await Promise.all([probeOnce("https://svc.test", 1, slow), probeOnce("https://other.test", 1, slow)]);
    expect(n).toBe(4);
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

  it("asks GET /healthz: the service's own answer is up; an error, a 5xx or no answer in time is down", async () => {
    const seen: string[] = [];
    const ok = (status: number) => (async (u: RequestInfo | URL) => (seen.push(String(u)), new Response(HEALTH, { status }))) as typeof fetch;
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
    expect(gateLine("not_render", "https://svc.test")).toBe("Renders are off: https://svc.test answered, but not as a Sawhorse render service (its /healthz did not name the service). Check the address.");
    expect(gateLine("ready", "https://svc.test")).toMatch(/^Renders are on/);
    expect(gateLine("checking", "https://svc.test")).toMatch(/^Asking/);
  });

  // Lane 20: a catch-all server (a static host, a router, a proxy's sign-in page) answers
  // 200 with HTML to every path. That is not the render service.
  it("takes only the render service's own JSON as up: a 200 with HTML or other JSON is not the service", async () => {
    const answer = (body: string, type = "application/json") => (async () => new Response(body, { status: 200, headers: { "Content-Type": type } })) as typeof fetch;
    expect(await checkService("https://svc.test", answer(HEALTH))).toBe("ok");
    expect(await checkService("https://svc.test", answer("<!doctype html><html><body>Welcome</body></html>", "text/html"))).toBe("other");
    expect(await checkService("https://svc.test", answer("{}"))).toBe("other");
    expect(await checkService("https://svc.test", answer(JSON.stringify({ ok: true })))).toBe("other");
    expect(await checkService("https://svc.test", answer(JSON.stringify({ service: "something-else", version: "1" })))).toBe("other");
    expect(await checkService("https://svc.test", answer(JSON.stringify({ service: "sawhorse-render" })))).toBe("other");
    expect(await checkService("https://svc.test", answer(JSON.stringify({ service: "sawhorse-render", version: "" })))).toBe("other");
    expect(await checkService("https://svc.test", answer("null"))).toBe("other");
    expect(await checkService("https://svc.test", answer(`{"service":"sawhorse-render","version":"1","pad":"${"x".repeat(70_000)}"}`))).toBe("other");
    expect(await serviceHealthy("https://svc.test", answer("<html></html>", "text/html"))).toBe(false);
    expect(isRenderHealth({ service: "sawhorse-render", version: "0.1.0", ok: true })).toBe(true);
    expect(isRenderHealth([{ service: "sawhorse-render", version: "0.1.0" }])).toBe(false);
    expect(isRenderHealth({ service: "sawhorse-render", version: 1 })).toBe(false);
  });

  it("a catch-all server keeps renders off and Settings says it is not the service", async () => {
    saveServiceUrl("https://catchall.test");
    const html = (async () => new Response("<!doctype html><title>Home</title>", { status: 200, headers: { "Content-Type": "text/html" } })) as typeof fetch;
    const g = renderHook(() => useRenderGate(html));
    await waitFor(() => expect(g.result.current.status).toBe("not_render"));
    g.unmount();
  });
});
