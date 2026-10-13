// A zip's central directory, read without inflating anything (lane 16): names, methods,
// declared sizes and CRCs, so a reader can refuse a zip bomb, a duplicate name or an entry
// that is not what its header says before (or right after) it inflates a single byte.
// Plain APPNOTE 6.3: the end-of-central-directory record, then each central header. ZIP64
// sizes are read from the extra field. Nothing is allocated per entry beyond its record.

export interface ZipDirEntry {
  name: string;
  /** 0 stored, 8 deflate. */
  method: number;
  compressedSize: number;
  /** The uncompressed size the directory declares. */
  size: number;
  crc: number;
  /** Offset of the entry's local header. */
  offset: number;
}

export class ZipDirError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ZipDirError";
  }
}

const u16 = (b: Uint8Array, o: number) => b[o] | (b[o + 1] << 8);
const u32 = (b: Uint8Array, o: number) => (b[o] | (b[o + 1] << 8) | (b[o + 2] << 16) | (b[o + 3] << 24)) >>> 0;
const u64 = (b: Uint8Array, o: number) => u32(b, o) + u32(b, o + 4) * 2 ** 32;

/** The central directory of `bytes`, refusing more than `maxEntries` entries before reading them. */
export function zipDirectory(bytes: Uint8Array, maxEntries: number): ZipDirEntry[] {
  // The end record is in the last 22 + 65,535 bytes (its comment is at most 64 KiB).
  let eocd = -1;
  for (let o = bytes.length - 22; o >= Math.max(0, bytes.length - 22 - 65535); o--) {
    if (u32(bytes, o) === 0x06054b50) {
      eocd = o;
      break;
    }
  }
  if (eocd < 0) throw new ZipDirError("not a zip (no end of central directory)");
  let count = u16(bytes, eocd + 10);
  let dirSize = u32(bytes, eocd + 12);
  let dirStart = u32(bytes, eocd + 16);
  // ZIP64: the locator sits just before the end record.
  if ((count === 0xffff || dirStart === 0xffffffff || dirSize === 0xffffffff) && eocd >= 20 && u32(bytes, eocd - 20) === 0x07064b50) {
    const rec = u64(bytes, eocd - 12);
    if (rec + 56 > bytes.length || u32(bytes, rec) !== 0x06064b50) throw new ZipDirError("a damaged ZIP64 directory");
    count = u64(bytes, rec + 32);
    dirSize = u64(bytes, rec + 40);
    dirStart = u64(bytes, rec + 48);
  }
  if (count > maxEntries) throw new ZipDirError(`the zip holds ${count} entries (at most ${maxEntries})`);
  if (dirStart + dirSize > bytes.length) throw new ZipDirError("the zip's directory runs past the end of the file");
  const out: ZipDirEntry[] = [];
  const decoder = new TextDecoder();
  let o = dirStart;
  for (let i = 0; i < count; i++) {
    if (o + 46 > bytes.length || u32(bytes, o) !== 0x02014b50) throw new ZipDirError("a damaged zip directory");
    const method = u16(bytes, o + 10);
    const crc = u32(bytes, o + 16);
    let compressedSize = u32(bytes, o + 20);
    let size = u32(bytes, o + 24);
    const nameLen = u16(bytes, o + 28);
    const extraLen = u16(bytes, o + 30);
    const commentLen = u16(bytes, o + 32);
    let offset = u32(bytes, o + 42);
    if (o + 46 + nameLen + extraLen > bytes.length) throw new ZipDirError("a damaged zip directory");
    const name = decoder.decode(bytes.subarray(o + 46, o + 46 + nameLen));
    // ZIP64 extra field (id 1): the 0xffffffff fields, in order.
    let e = o + 46 + nameLen;
    const end = e + extraLen;
    while (e + 4 <= end) {
      const id = u16(bytes, e);
      const len = u16(bytes, e + 2);
      if (id === 1) {
        let f = e + 4;
        if (size === 0xffffffff && f + 8 <= e + 4 + len) (size = u64(bytes, f)), (f += 8);
        if (compressedSize === 0xffffffff && f + 8 <= e + 4 + len) (compressedSize = u64(bytes, f)), (f += 8);
        if (offset === 0xffffffff && f + 8 <= e + 4 + len) offset = u64(bytes, f);
      }
      e += 4 + len;
    }
    out.push({ name, method, compressedSize, size, crc, offset });
    o += 46 + nameLen + extraLen + commentLen;
  }
  return out;
}

let table: Uint32Array | null = null;

/** CRC-32 (the zip one, IEEE 802.3). */
export function crc32(data: Uint8Array): number {
  if (!table) {
    table = new Uint32Array(256);
    for (let n = 0; n < 256; n++) {
      let c = n;
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      table[n] = c >>> 0;
    }
  }
  let c = 0xffffffff;
  for (let i = 0; i < data.length; i++) c = table[(c ^ data[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
