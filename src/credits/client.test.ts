import { describe, expect, it } from "vitest";
import { RenderServiceClient, ServiceError, ServiceUnreachableError, type FetchLike } from "./client";
import { b64url, canonicalString, fromB64url, installIdFor, type DeviceKey } from "./device";
import { vectorKey } from "./vector.test";

async function testKey(): Promise<DeviceKey & { verify(sig: Uint8Array, msg: Uint8Array): Promise<boolean> }> {
  const pair = (await crypto.subtle.generateKey({ name: "Ed25519" }, false, ["sign", "verify"])) as CryptoKeyPair;
  const pub = new Uint8Array(await crypto.subtle.exportKey("raw", pair.publicKey));
  return {
    installId: await installIdFor(pub),
    publicKey: pub,
    storage: "session-only",
    sign: async (m) => new Uint8Array(await crypto.subtle.sign("Ed25519", pair.privateKey, m as BufferSource)),
    verify: async (sig, msg) => crypto.subtle.verify("Ed25519", pair.publicKey, sig as BufferSource, msg as BufferSource),
  };
}

interface Seen {
  url: string;
  method: string;
  headers: Record<string, string>;
  body: Uint8Array;
}

function fake(handler: (req: Seen) => Response | Promise<Response>): { f: FetchLike; seen: Seen[] } {
  const seen: Seen[] = [];
  const f: FetchLike = async (url, init) => {
    const req: Seen = { url, method: init?.method ?? "GET", headers: (init?.headers ?? {}) as Record<string, string>, body: (init?.body as Uint8Array | undefined) ?? new Uint8Array() };
    seen.push(req);
    return handler(req);
  };
  return { f, seen };
}

describe("RenderServiceClient", () => {
  it("signs method, path with query, body hash, timestamp and nonce", async () => {
    const key = await testKey();
    const { f, seen } = fake(() => Response.json({ renders: [] }));
    const c = new RenderServiceClient("http://svc.test/", async () => key, f);
    await c.history(7);
    const r = seen[0];
    expect(r.url).toBe("http://svc.test/v1/renders?limit=7");
    expect(r.headers["X-Install-Id"]).toBe(key.installId);
    expect(fromB64url(r.headers["X-Public-Key"])).toEqual(key.publicKey);
    expect(Math.abs(Number(r.headers["X-Timestamp"]) - Date.now() / 1000)).toBeLessThan(5);
    expect(r.headers["X-Nonce"]).toMatch(/^[A-Za-z0-9_-]{22}$/);
    const msg = await canonicalString("GET", "/v1/renders?limit=7", "", r.headers["X-Timestamp"], r.headers["X-Nonce"]);
    expect(await key.verify(fromB64url(r.headers["X-Signature"]), new TextEncoder().encode(msg))).toBe(true);

    await c.checkout("p250");
    const p = seen[1];
    expect(new TextDecoder().decode(p.body)).toBe('{"pack":"p250"}');
    expect(p.headers["Content-Type"]).toBe("application/json");
    const pmsg = await canonicalString("POST", "/v1/checkout", p.body, p.headers["X-Timestamp"], p.headers["X-Nonce"]);
    expect(await key.verify(fromB64url(p.headers["X-Signature"]), new TextEncoder().encode(pmsg))).toBe(true);
    expect(p.headers["X-Nonce"]).not.toBe(r.headers["X-Nonce"]);
  });

  it("produces the shared vector's exact signature for the vector's request", async () => {
    const { privateKey, publicKey } = await vectorKey();
    const msg = await canonicalString("POST", "/v1/checkout", new TextEncoder().encode('{"pack":"p100"}'), "1760140800", "AAAAAAAAAAAAAAAAAAAAAA");
    const sig = new Uint8Array(await crypto.subtle.sign("Ed25519", privateKey, new TextEncoder().encode(msg)));
    expect(b64url(sig)).toBe("24sJ6AYzwwh1SduKox6hs3dTTm83RQTODQVsPbsl3bb4Bta0tkXu2ioHAprJmec2-2A_YCBgj9wFwOoN8FOtDg");
    expect(b64url(publicKey)).toBe("A6EHv_POEL4dcN0Y50vAmWfk1jCbpQ1fHdyGZBJVMbg");
  });

  it("maps errors: unreachable, typed service errors with Retry-After", async () => {
    const key = await testKey();
    const down = new RenderServiceClient("http://svc.test", async () => key, async () => {
      throw new TypeError("Failed to fetch");
    });
    await expect(down.balance()).rejects.toBeInstanceOf(ServiceUnreachableError);
    await expect(down.balance()).rejects.toThrow(/not connected \(http:\/\/svc\.test\)/);

    const { f } = fake(() => Response.json({ error: "rate_limited", message: "Limit reached", limit: "per_minute", balance: 4 }, { status: 429, headers: { "Retry-After": "42" } }));
    const limited = new RenderServiceClient("http://svc.test", async () => key, f);
    const err = (await limited.balance().catch((e) => e)) as ServiceError;
    expect(err).toBeInstanceOf(ServiceError);
    expect(err.status).toBe(429);
    expect(err.code).toBe("rate_limited");
    expect(err.body).toMatchObject({ retry_after: 42, balance: 4, limit: "per_minute" });
  });

  it("corrects a skewed clock once from the server's Date header", async () => {
    const key = await testKey();
    const serverNow = Date.now() + 3600_000;
    let calls = 0;
    const { f, seen } = fake((req) => {
      calls++;
      const skew = Math.abs(Number(req.headers["X-Timestamp"]) - serverNow / 1000);
      if (skew > 300) return Response.json({ error: "bad_signature", message: "Clock skew 3600 s is over 300 s." }, { status: 401, headers: { Date: new Date(serverNow).toUTCString() } });
      return Response.json({ install_id: key.installId, credits: 3, devices: 1, max_devices: 2, render_cost: 1, packs: [] });
    });
    const c = new RenderServiceClient("http://svc.test", async () => key, f);
    expect((await c.balance()).credits).toBe(3);
    expect(calls).toBe(2);
    expect(seen[0].headers["X-Nonce"]).not.toBe(seen[1].headers["X-Nonce"]);
    // The offset sticks: the next call is right the first time.
    await c.balance();
    expect(calls).toBe(3);
  });
});
