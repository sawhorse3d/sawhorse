// A small PNG writer for the render passes. Canvas can only export 8-bit PNGs; the depth pass
// needs 16 bits, so we write the file ourselves. Deflate comes from the platform
// (CompressionStream "deflate" is the zlib format PNG's IDAT wants).

const SIGNATURE = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

let crcTable: Uint32Array | null = null;
export function crc32(bytes: Uint8Array, crc = 0xffffffff): number {
  if (!crcTable) {
    crcTable = new Uint32Array(256);
    for (let n = 0; n < 256; n++) {
      let c = n;
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      crcTable[n] = c >>> 0;
    }
  }
  for (let i = 0; i < bytes.length; i++) crc = crcTable[(crc ^ bytes[i]) & 0xff] ^ (crc >>> 8);
  return crc;
}

async function deflate(data: Uint8Array): Promise<Uint8Array> {
  const cs = new CompressionStream("deflate");
  const writer = cs.writable.getWriter();
  // Not awaited: the readable side must drain while we write (back-pressure).
  void writer.write(data as Uint8Array<ArrayBuffer>);
  void writer.close();
  return new Uint8Array(await new Response(cs.readable).arrayBuffer());
}

function chunk(type: string, data: Uint8Array): Uint8Array {
  const out = new Uint8Array(12 + data.length);
  const view = new DataView(out.buffer);
  view.setUint32(0, data.length);
  for (let i = 0; i < 4; i++) out[4 + i] = type.charCodeAt(i);
  out.set(data, 8);
  view.setUint32(8 + data.length, (crc32(out.subarray(4, 8 + data.length)) ^ 0xffffffff) >>> 0);
  return out;
}

export interface GreyImage {
  width: number;
  height: number;
  /** Row-major samples: Uint16Array for 16-bit, Uint8Array for 8-bit. */
  data: Uint16Array | Uint8Array;
}

/** Encode a greyscale image (bit depth from the array type) as a PNG, Sub filter on every row. */
export async function encodeGreyPng(img: GreyImage): Promise<Uint8Array> {
  const { width, height, data } = img;
  const depth = data instanceof Uint16Array ? 16 : 8;
  const bpp = depth / 8;
  const stride = width * bpp;
  const raw = new Uint8Array(height * (stride + 1));
  for (let y = 0; y < height; y++) {
    const row = y * (stride + 1);
    raw[row] = 1; // Sub
    let prevHi = 0;
    let prevLo = 0;
    for (let x = 0; x < width; x++) {
      const v = data[y * width + x];
      if (depth === 16) {
        const hi = (v >> 8) & 255;
        const lo = v & 255;
        raw[row + 1 + 2 * x] = (hi - prevHi) & 255;
        raw[row + 2 + 2 * x] = (lo - prevLo) & 255;
        prevHi = hi;
        prevLo = lo;
      } else {
        raw[row + 1 + x] = (v - prevHi) & 255;
        prevHi = v;
      }
    }
  }
  const ihdr = new Uint8Array(13);
  const v = new DataView(ihdr.buffer);
  v.setUint32(0, width);
  v.setUint32(4, height);
  ihdr[8] = depth;
  ihdr[9] = 0; // greyscale
  const parts = [SIGNATURE, chunk("IHDR", ihdr), chunk("IDAT", await deflate(raw)), chunk("IEND", new Uint8Array(0))];
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let off = 0;
  for (const p of parts) {
    out.set(p, off);
    off += p.length;
  }
  return out;
}

export function bytesToBase64(bytes: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(bin);
}
