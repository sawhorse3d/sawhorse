// The signing vector shared with the render service (render-service/test/vector.test.ts).
// Same seed, same request, same bytes: if either side drifts, both suites fail.
import { describe, expect, it } from "vitest";
import { b64url, base32, canonicalString, fromB64url, installIdFor, toHex, sha256 } from "./device";

export const VECTOR = {
  seedHex: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
  publicKey: "A6EHv_POEL4dcN0Y50vAmWfk1jCbpQ1fHdyGZBJVMbg",
  installId: "kzdvvj2umnduyauf35o3",
  method: "POST",
  path: "/v1/checkout",
  body: '{"pack":"p100"}',
  bodySha256: "96d8e19549877ae78a69219aea38f01653b12016ff1030bb45d6b9b780d6fcc2",
  timestamp: "1760140800",
  nonce: "AAAAAAAAAAAAAAAAAAAAAA",
  canonical: "sawhorse-v1\nPOST\n/v1/checkout\n96d8e19549877ae78a69219aea38f01653b12016ff1030bb45d6b9b780d6fcc2\n1760140800\nAAAAAAAAAAAAAAAAAAAAAA",
  signature: "24sJ6AYzwwh1SduKox6hs3dTTm83RQTODQVsPbsl3bb4Bta0tkXu2ioHAprJmec2-2A_YCBgj9wFwOoN8FOtDg",
};

const PKCS8_PREFIX = "302e020100300506032b657004220420";

function hex(s: string): Uint8Array {
  return new Uint8Array(s.match(/../g)!.map((h) => parseInt(h, 16)));
}

/** WebCrypto key from the vector's seed (the same API the browser demo signs with). */
export async function vectorKey(): Promise<{ privateKey: CryptoKey; publicKey: Uint8Array }> {
  const privateKey = await crypto.subtle.importKey("pkcs8", hex(PKCS8_PREFIX + VECTOR.seedHex) as BufferSource, { name: "Ed25519" }, true, ["sign"]);
  const jwk = await crypto.subtle.exportKey("jwk", privateKey);
  return { privateKey, publicKey: fromB64url(jwk.x!) };
}

describe("shared signing vector (WebCrypto)", () => {
  it("derives the same public key and install id", async () => {
    const { publicKey } = await vectorKey();
    expect(b64url(publicKey)).toBe(VECTOR.publicKey);
    expect(await installIdFor(publicKey)).toBe(VECTOR.installId);
  });

  it("builds the same canonical string", async () => {
    expect(toHex(await sha256(VECTOR.body))).toBe(VECTOR.bodySha256);
    expect(await canonicalString(VECTOR.method, VECTOR.path, new TextEncoder().encode(VECTOR.body), VECTOR.timestamp, VECTOR.nonce)).toBe(VECTOR.canonical);
  });

  it("signs to the same bytes", async () => {
    const { privateKey } = await vectorKey();
    const sig = new Uint8Array(await crypto.subtle.sign("Ed25519", privateKey, new TextEncoder().encode(VECTOR.canonical)));
    expect(b64url(sig)).toBe(VECTOR.signature);
  });

  it("base32 and base64url agree with the service", () => {
    expect(base32(new TextEncoder().encode("foobar"))).toBe("mzxw6ytboi");
    expect(base32(new Uint8Array(40).fill(255))).toBe("7".repeat(64));
    const bytes = new Uint8Array(70000).map((_, i) => i * 7);
    expect(Array.from(fromB64url(b64url(bytes)))).toEqual(Array.from(bytes));
  });
});
