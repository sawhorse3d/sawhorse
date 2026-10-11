import { describe, expect, it } from "vitest";
import { bytesToBase64, crc32, encodeGreyPng } from "./png";
import { cameraFor, passSize } from "./passes";
import * as THREE from "three";

async function inflate(data: Uint8Array): Promise<Uint8Array> {
  const ds = new DecompressionStream("deflate");
  const w = ds.writable.getWriter();
  void w.write(data as Uint8Array<ArrayBuffer>);
  void w.close();
  return new Uint8Array(await new Response(ds.readable).arrayBuffer());
}

function u32(b: Uint8Array, off: number): number {
  return new DataView(b.buffer, b.byteOffset, b.byteLength).getUint32(off);
}

/** Decode our own greyscale PNGs (Sub filter) back to samples, checking every chunk CRC. */
async function decode(png: Uint8Array): Promise<{ width: number; height: number; depth: number; samples: number[]; chunks: string[] }> {
  expect(Array.from(png.subarray(0, 8))).toEqual([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const chunks: string[] = [];
  let off = 8;
  let width = 0;
  let height = 0;
  let depth = 0;
  const idat: number[] = [];
  while (off < png.length) {
    const len = u32(png, off);
    const type = String.fromCharCode(...png.subarray(off + 4, off + 8));
    const data = png.subarray(off + 8, off + 8 + len);
    expect(u32(png, off + 8 + len)).toBe((crc32(png.subarray(off + 4, off + 8 + len)) ^ 0xffffffff) >>> 0);
    chunks.push(type);
    if (type === "IHDR") {
      width = u32(data, 0);
      height = u32(data, 4);
      depth = data[8];
      expect(data[9]).toBe(0);
    }
    if (type === "IDAT") idat.push(...data);
    off += 12 + len;
  }
  const raw = await inflate(new Uint8Array(idat));
  const bpp = depth / 8;
  const stride = width * bpp;
  const out = new Uint8Array(height * stride);
  for (let y = 0; y < height; y++) {
    expect(raw[y * (stride + 1)]).toBe(1);
    for (let x = 0; x < stride; x++) out[y * stride + x] = (raw[y * (stride + 1) + 1 + x] + (x >= bpp ? out[y * stride + x - bpp] : 0)) & 255;
  }
  const samples: number[] = [];
  for (let i = 0; i < width * height; i++) samples.push(depth === 16 ? (out[2 * i] << 8) | out[2 * i + 1] : out[i]);
  return { width, height, depth, samples, chunks };
}

describe("PNG writer", () => {
  it("writes 16-bit greyscale that decodes to the same samples", async () => {
    const data = new Uint16Array([0, 1, 255, 256, 65535, 30000, 8192, 12345, 54321, 2, 3, 4]);
    const png = await decode(await encodeGreyPng({ width: 4, height: 3, data }));
    expect([png.width, png.height, png.depth]).toEqual([4, 3, 16]);
    expect(png.samples).toEqual(Array.from(data));
    expect(png.chunks).toEqual(["IHDR", "IDAT", "IEND"]);
  });

  it("writes 8-bit greyscale", async () => {
    const data = new Uint8Array(64 * 40).map((_, i) => (i * 13) & 255);
    const png = await decode(await encodeGreyPng({ width: 64, height: 40, data }));
    expect(png.depth).toBe(8);
    expect(png.samples).toEqual(Array.from(data));
  });

  it("crc32 matches the PNG spec's IEND constant", () => {
    expect((crc32(new TextEncoder().encode("IEND")) ^ 0xffffffff) >>> 0).toBe(0xae426082);
  });

  it("base64 handles large buffers", () => {
    const bytes = new Uint8Array(200_000).map((_, i) => i);
    const back = atob(bytesToBase64(bytes));
    expect(back.length).toBe(bytes.length);
    expect(back.charCodeAt(199_999)).toBe(199_999 & 255);
  });
});

describe("pass geometry", () => {
  it("keeps the view's aspect, long side 1024, multiples of 16", () => {
    expect(passSize(1084, 744)).toEqual({ width: 1024, height: 704 });
    expect(passSize(600, 900)).toEqual({ width: 688, height: 1024 });
    expect(passSize(0, 0)).toEqual({ width: 1024, height: 640 });
    for (const [w, h] of [[1440, 900], [377, 1200], [1920, 300]]) {
      const s = passSize(w, h);
      expect(s.width % 16).toBe(0);
      expect(s.height % 16).toBe(0);
      expect(Math.max(s.width, s.height)).toBeLessThanOrEqual(1024);
    }
  });

  it("re-aims the camera at the output aspect without moving it", () => {
    const ortho = new THREE.OrthographicCamera(-2, 2, 1, -1, 0.1, 100);
    ortho.position.set(1, 2, 3);
    const o = cameraFor(ortho, 3) as THREE.OrthographicCamera;
    expect([o.left, o.right, o.top, o.bottom]).toEqual([-3, 3, 1, -1]);
    expect(o.position.toArray()).toEqual([1, 2, 3]);
    expect(ortho.left).toBe(-2);
    const persp = new THREE.PerspectiveCamera(35, 1.5, 0.1, 100);
    expect((cameraFor(persp, 2) as THREE.PerspectiveCamera).aspect).toBe(2);
    expect(persp.aspect).toBe(1.5);
  });
});
