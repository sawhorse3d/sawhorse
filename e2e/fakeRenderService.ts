// A stand-in for the render service (render-service/, a separate repo) inside Playwright:
// every request the app makes to http://127.0.0.1:8787 is answered here, and every signed
// request is verified exactly as the service verifies it (install id from the key, Ed25519
// over the canonical string, 5-minute skew, nonce never reused). CI has no render service,
// so this is how the browser build's credit flow is tested end to end. The real service is
// exercised against the app by hand (docs/RENDER.md, "End to end locally").
import { createHash, createPublicKey, verify } from "node:crypto";
import { crc32, deflateSync, inflateSync } from "node:zlib";
import type { Page, Route } from "@playwright/test";

export const SERVICE = "http://127.0.0.1:8787";

const B32 = "abcdefghijklmnopqrstuvwxyz234567";
function base32(bytes: Uint8Array): string {
  let out = "";
  let bits = 0;
  let value = 0;
  for (const b of bytes) {
    value = ((value << 8) | b) & 0xffff;
    bits += 8;
    while (bits >= 5) {
      out += B32[(value >>> (bits - 5)) & 31];
      bits -= 5;
    }
  }
  if (bits > 0) out += B32[(value << (5 - bits)) & 31];
  return out;
}

export interface DecodedPng {
  width: number;
  height: number;
  bitDepth: number;
  colorType: number;
  samples: number[];
}

/** Decode a non-interlaced greyscale PNG (what the app sends). */
export function decodeGreyPng(buf: Buffer): DecodedPng {
  const width = buf.readUInt32BE(16);
  const height = buf.readUInt32BE(20);
  const bitDepth = buf[24];
  const colorType = buf[25];
  const idat: Buffer[] = [];
  let off = 8;
  while (off < buf.length) {
    const len = buf.readUInt32BE(off);
    const type = buf.toString("latin1", off + 4, off + 8);
    if (type === "IDAT") idat.push(buf.subarray(off + 8, off + 8 + len));
    off += 12 + len;
  }
  const raw = inflateSync(Buffer.concat(idat));
  const bpp = bitDepth / 8;
  const stride = width * bpp;
  const out = new Uint8Array(height * stride);
  for (let y = 0; y < height; y++) {
    const f = raw[y * (stride + 1)];
    for (let x = 0; x < stride; x++) {
      const v = raw[y * (stride + 1) + 1 + x];
      const a = x >= bpp ? out[y * stride + x - bpp] : 0;
      const up = y > 0 ? out[(y - 1) * stride + x] : 0;
      out[y * stride + x] = (f === 1 ? v + a : f === 2 ? v + up : v) & 255;
    }
  }
  const samples: number[] = [];
  for (let i = 0; i < width * height; i++) samples.push(bitDepth === 16 ? (out[2 * i] << 8) | out[2 * i + 1] : out[i]);
  return { width, height, bitDepth, colorType, samples };
}

/** A tiny RGB PNG: the "render" (a warm gradient). */
function resultPng(): Buffer {
  const w = 64;
  const h = 40;
  const raw = Buffer.alloc(h * (w * 3 + 1));
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) raw.set([200 - y * 2, 150 - y, 90 + x], y * (w * 3 + 1) + 1 + x * 3);
  const chunk = (type: string, data: Buffer) => {
    const head = Buffer.alloc(8);
    head.writeUInt32BE(data.length);
    head.write(type, 4, "latin1");
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(Buffer.concat([head.subarray(4), data])) >>> 0);
    return Buffer.concat([head, data, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8;
  ihdr[9] = 2;
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", ihdr), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

export interface FakeService {
  credits: number;
  installIds: Set<string>;
  nonces: Set<string>;
  renders: { prompt: string; preset: string | null; depth: DecodedPng; edges: DecodedPng; clay: DecodedPng }[];
  checkouts: string[];
  rejected: string[];
}

export async function fakeRenderService(page: Page, initialCredits = 3): Promise<FakeService> {
  const state: FakeService = { credits: initialCredits, installIds: new Set(), nonces: new Set(), renders: [], checkouts: [], rejected: [] };
  const png = resultPng();
  const cors = { "Access-Control-Allow-Origin": "*", "Access-Control-Allow-Headers": "*", "Access-Control-Allow-Methods": "GET, POST, OPTIONS" };
  const json = (route: Route, status: number, body: unknown) => route.fulfill({ status, headers: { ...cors, "Content-Type": "application/json" }, body: JSON.stringify(body) });

  await page.route(`${SERVICE}/**`, async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: cors });
    // Lane 15: the app asks /healthz at startup before it shows the chip and the Render pane;
    // lane 20: it must be the service's own answer, which names the service.
    if (url.pathname === "/healthz") return json(route, 200, { ok: true, service: "sawhorse-render", version: "0.1.0-test", mock: true });
    if (url.pathname.endsWith("/image")) return route.fulfill({ status: 200, headers: { ...cors, "Content-Type": "image/png" }, body: png });

    // Verify the signature the way the service does.
    const h = req.headers();
    const body = req.postDataBuffer() ?? Buffer.alloc(0);
    const pub = Buffer.from(h["x-public-key"] ?? "", "base64url");
    const id = base32(createHash("sha256").update(pub).digest()).slice(0, 20);
    const message = ["sawhorse-v1", req.method(), url.pathname + url.search, createHash("sha256").update(body).digest("hex"), h["x-timestamp"], h["x-nonce"]].join("\n");
    let ok = pub.length === 32 && id === h["x-install-id"] && Math.abs(Date.now() / 1000 - Number(h["x-timestamp"])) < 300 && !state.nonces.has(h["x-nonce"] ?? "");
    if (ok) {
      const key = createPublicKey({ key: { kty: "OKP", crv: "Ed25519", x: pub.toString("base64url") }, format: "jwk" });
      ok = verify(null, Buffer.from(message), key, Buffer.from(h["x-signature"] ?? "", "base64url"));
    }
    if (!ok) {
      state.rejected.push(url.pathname);
      return json(route, 401, { error: "bad_signature", message: "Signature does not verify." });
    }
    state.nonces.add(h["x-nonce"]!);
    state.installIds.add(id);

    if (url.pathname === "/v1/balance") return json(route, 200, { install_id: id, credits: state.credits, devices: 1, max_devices: 2, render_cost: 1, packs: [{ id: "p100", credits: 100, amount: 1000, currency: "usd", label: "$10" }, { id: "p250", credits: 250, amount: 2500, currency: "usd", label: "$25" }, { id: "p500", credits: 500, amount: 5000, currency: "usd", label: "$50" }] });
    if (url.pathname === "/v1/checkout") {
      const { pack } = JSON.parse(body.toString()) as { pack: string };
      state.checkouts.push(pack);
      return json(route, 200, { session_id: "cs_test_e2e", url: `${SERVICE}/dev/checkout/cs_test_e2e`, mode: "simulated", credits: 100 });
    }
    if (url.pathname === "/v1/renders") return json(route, 200, { renders: [] });
    if (url.pathname === "/v1/render") {
      const r = JSON.parse(body.toString()) as { prompt: string; preset: string | null; depth_png: string; edges_png: string; clay_png: string; request_id: string };
      if (state.credits < 1) return json(route, 402, { error: "insufficient_credits", message: "No credits", balance: 0 });
      state.credits--;
      state.renders.push({
        prompt: r.prompt,
        preset: r.preset,
        depth: decodeGreyPng(Buffer.from(r.depth_png, "base64")),
        edges: decodeGreyPng(Buffer.from(r.edges_png, "base64")),
        clay: decodeGreyPng(Buffer.from(r.clay_png, "base64")),
      });
      return json(route, 200, { request_id: r.request_id, status: "succeeded", provider: "mock", mock: true, width: 64, height: 40, image_url: `${SERVICE}/v1/renders/${r.request_id}/image?exp=1&sig=x`, expires_at: new Date(Date.now() + 9e5).toISOString(), balance: state.credits });
    }
    return json(route, 404, { error: "not_found", message: url.pathname });
  });
  return state;
}
