// Test helpers (lane 16): zip bombs built in milliseconds and zips with any declared sizes,
// for the readers that must refuse them before inflating. Node only (tests).
import { constants, deflateRawSync } from "node:zlib";

/** A raw Deflate stream inflating to `n` zero bytes (a multiple of 64 MiB, or less than
 * that): one fully flushed run of blocks repeated, then a final empty block. About 1 KB
 * per MiB. */
export function deflateZeros(n: number): Uint8Array {
  const chunk = Math.min(n, 64 * 1024 * 1024);
  const block = deflateRawSync(Buffer.alloc(chunk), { level: 9, finishFlush: constants.Z_FULL_FLUSH });
  const times = Math.max(1, Math.floor(n / chunk));
  const out = new Uint8Array(block.length * times + 2);
  for (let i = 0; i < times; i++) out.set(block, i * block.length);
  out.set([0x03, 0x00], block.length * times);
  return out;
}

export interface RawEntry {
  name: string;
  /** 0 stored, 8 deflate. */
  method: number;
  data: Uint8Array;
  /** What the directory says the entry inflates to. */
  size: number;
  crc?: number;
}

/** A zip whose directory says what it is told (sizes, CRCs), whatever the data is. */
export function rawZip(entries: RawEntry[]): Uint8Array {
  const parts: number[] = [];
  const cd: number[] = [];
  const u16 = (a: number[], v: number) => a.push(v & 255, (v >>> 8) & 255);
  const u32 = (a: number[], v: number) => a.push(v & 255, (v >>> 8) & 255, (v >>> 16) & 255, (v >>> 24) & 255);
  const bytes = (a: number[], b: Uint8Array) => {
    for (let i = 0; i < b.length; i++) a.push(b[i]);
  };
  for (const e of entries) {
    const name = new TextEncoder().encode(e.name);
    const off = parts.length;
    u32(parts, 0x04034b50);
    u16(parts, 20);
    u16(parts, 0);
    u16(parts, e.method);
    u32(parts, 0);
    u32(parts, e.crc ?? 0);
    u32(parts, e.data.length);
    u32(parts, e.size);
    u16(parts, name.length);
    u16(parts, 0);
    bytes(parts, name);
    bytes(parts, e.data);
    u32(cd, 0x02014b50);
    u16(cd, 20);
    u16(cd, 20);
    u16(cd, 0);
    u16(cd, e.method);
    u32(cd, 0);
    u32(cd, e.crc ?? 0);
    u32(cd, e.data.length);
    u32(cd, e.size);
    u16(cd, name.length);
    for (let i = 0; i < 12; i++) cd.push(0);
    u32(cd, off);
    bytes(cd, name);
  }
  const cdOff = parts.length;
  const out = [...parts, ...cd];
  u32(out, 0x06054b50);
  u32(out, 0);
  u16(out, entries.length);
  u16(out, entries.length);
  u32(out, cd.length);
  u32(out, cdOff);
  u16(out, 0);
  return Uint8Array.from(out);
}
