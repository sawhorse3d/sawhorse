// Device identity for render credits (ADR-0004). No accounts: each install has an Ed25519
// keypair, and the render service knows the device only by its public key.
//
//   install_id = base32(sha256(raw 32-byte public key)), lowercase RFC 4648, no padding, [:20]
//   canonical  = "sawhorse-v1\n" + METHOD + "\n" + path?query + "\n"
//                + hex(sha256(body bytes)) + "\n" + timestamp + "\n" + nonce
//
// The render service implements the same functions (render-service/src/crypto.ts); both
// sides test against the shared vector in ./vector.test.ts.
//
// Where the private key lives:
//   - Desktop (Tauri): in the OS secure store (Windows Credential Manager, macOS Keychain,
//     Secret Service on Linux) behind two Tauri commands; the key never enters the webview.
//   - Browser demo: a non-extractable WebCrypto key in IndexedDB. Script on this origin can
//     ask it to sign but cannot read the key out. Clearing site data loses it (and with it
//     the credits on that device, unless they are moved with a claim code first).
//   - No IndexedDB (private windows, blocked storage): a key for this page load only.

export const SIGNING_VERSION = "sawhorse-v1";
const B32 = "abcdefghijklmnopqrstuvwxyz234567";

export function base32(bytes: Uint8Array): string {
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

export function toHex(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += b.toString(16).padStart(2, "0");
  return s;
}

export function b64url(bytes: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function fromB64url(s: string): Uint8Array {
  const bin = atob(s.replace(/-/g, "+").replace(/_/g, "/") + "===".slice((s.length + 3) % 4));
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

const utf8 = new TextEncoder();

export async function sha256(data: Uint8Array | string): Promise<Uint8Array> {
  const bytes = typeof data === "string" ? utf8.encode(data) : data;
  return new Uint8Array(await crypto.subtle.digest("SHA-256", bytes as BufferSource));
}

export async function installIdFor(publicKey: Uint8Array): Promise<string> {
  return base32(await sha256(publicKey)).slice(0, 20);
}

export async function canonicalString(method: string, pathAndQuery: string, body: Uint8Array | string, timestamp: string, nonce: string): Promise<string> {
  return [SIGNING_VERSION, method.toUpperCase(), pathAndQuery, toHex(await sha256(body)), timestamp, nonce].join("\n");
}

// ---- the device key ----------------------------------------------------------------------

export type KeyStorage = "os-secure-store" | "indexeddb" | "session-only";

export interface DeviceKey {
  installId: string;
  publicKey: Uint8Array;
  storage: KeyStorage;
  sign(message: Uint8Array): Promise<Uint8Array>;
}

export class DeviceKeyUnavailableError extends Error {}

interface TauriInternals {
  invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
}

export function tauri(): TauriInternals | null {
  const w = globalThis as unknown as { __TAURI_INTERNALS__?: TauriInternals };
  return w.__TAURI_INTERNALS__ ?? null;
}

async function fromTauri(t: TauriInternals): Promise<DeviceKey> {
  const pub = fromB64url(await t.invoke<string>("device_public_key"));
  return {
    installId: await installIdFor(pub),
    publicKey: pub,
    storage: "os-secure-store",
    async sign(message) {
      return fromB64url(await t.invoke<string>("device_sign", { message: b64url(message) }));
    },
  };
}

// IndexedDB: one store, one record holding the CryptoKeyPair (structured-cloneable; the
// private key is non-extractable, so it cannot be read back out, only used).
const DB_NAME = "sawhorse-device";
const STORE = "keys";
const RECORD = "device-v1";

function idb(): IDBFactory | null {
  try {
    return typeof indexedDB === "undefined" ? null : indexedDB;
  } catch {
    return null;
  }
}

function request<T>(r: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error);
  });
}

async function openDb(factory: IDBFactory): Promise<IDBDatabase> {
  const r = factory.open(DB_NAME, 1);
  r.onupgradeneeded = () => r.result.createObjectStore(STORE);
  return request(r);
}

async function webKey(pair: CryptoKeyPair, storage: KeyStorage): Promise<DeviceKey> {
  const pub = new Uint8Array(await crypto.subtle.exportKey("raw", pair.publicKey));
  return {
    installId: await installIdFor(pub),
    publicKey: pub,
    storage,
    async sign(message) {
      return new Uint8Array(await crypto.subtle.sign("Ed25519", pair.privateKey, message as BufferSource));
    },
  };
}

async function generate(): Promise<CryptoKeyPair> {
  try {
    return (await crypto.subtle.generateKey({ name: "Ed25519" }, false, ["sign", "verify"])) as CryptoKeyPair;
  } catch {
    throw new DeviceKeyUnavailableError("This browser cannot make Ed25519 keys (WebCrypto). Use a current Chrome, Edge, Firefox or Safari, or the desktop app.");
  }
}

async function fromBrowser(): Promise<DeviceKey> {
  const factory = idb();
  if (factory) {
    try {
      const db = await openDb(factory);
      try {
        const existing = await request(db.transaction(STORE, "readonly").objectStore(STORE).get(RECORD)) as CryptoKeyPair | undefined;
        if (existing?.privateKey && existing.publicKey) return await webKey(existing, "indexeddb");
        const pair = await generate();
        await request(db.transaction(STORE, "readwrite").objectStore(STORE).put(pair, RECORD));
        return await webKey(pair, "indexeddb");
      } finally {
        db.close();
      }
    } catch (err) {
      if (err instanceof DeviceKeyUnavailableError) throw err;
      /* storage blocked: fall through to a key for this page load */
    }
  }
  return webKey(await generate(), "session-only");
}

let cached: Promise<DeviceKey> | null = null;

/** The device key: created on first use, then the same key every time. */
export function deviceKey(): Promise<DeviceKey> {
  if (!cached) {
    const t = tauri();
    cached = (t ? fromTauri(t) : fromBrowser()).catch((err) => {
      cached = null;
      throw err;
    });
  }
  return cached;
}

/** Tests only. */
export function resetDeviceKeyCache(): void {
  cached = null;
}

export function storageNote(s: KeyStorage): string {
  switch (s) {
    case "os-secure-store":
      return "Device key in the system keychain.";
    case "indexeddb":
      return "Browser demo: the device key lives in this browser's site data. Clearing it loses the credits here; move them first with a claim code.";
    case "session-only":
      return "This browser blocks storage, so the device key lasts only until the tab closes. Use the desktop app or allow site data before buying credits.";
  }
}
