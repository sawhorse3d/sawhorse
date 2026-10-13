// What a plugin's Web Worker can reach. Run by the sandbox before any plugin code loads.
//
// A dedicated worker already has no DOM, no window and no access to the app's memory; it
// talks to the app only by messages, and the app checks every message against the
// plugin's permissions. What is left to take away is the worker's own reach outside the
// app: the network, storage the app does not mediate, more workers, channels to the app's
// other pages, importScripts, and every way to turn a string into code (eval, Function and
// its async and generator cousins, string timers), which would get round the static check
// of the plugin's sources.
//
// The global object is cut down to an ALLOWLIST: the language's own built-ins and a short
// list of web APIs that compute without reaching anything (text encoding, URLs, streams,
// crypto, timers, blobs, offscreen canvas ...). Everything else the browser puts on a
// worker's global object, and on the prototypes it inherits from, is deleted. An earlier
// version removed a BLOCKLIST of names instead; it missed `FontFace` (whose load() fetches
// any URL, so a plugin without "net" could send the model anywhere) and
// `webkitRequestFileSystemSync` (an origin-wide file system any plugin could write to), and
// every new browser API would have been one more hole. With an allowlist a new API is
// absent until someone decides it is safe. `navigator` is cut down the same way.
//
// This is one of three locks. The sources are parsed and checked before they run
// (packages/plugin-api/src/source.ts), and the worker is started from a blob: URL so it
// runs under the page's content security policy (host.ts, vite.config.ts), which refuses
// scripts and fonts from anywhere but the app itself.

/**
 * What a plugin's global object keeps. ECMAScript's built-ins, then web APIs that cannot
 * reach the network, storage, other pages or other threads. Anything not here is removed.
 */
export const ALLOWED_GLOBALS: ReadonlySet<string> = new Set([
  // ECMAScript
  "globalThis", "Infinity", "NaN", "undefined", "eval", "isFinite", "isNaN", "parseFloat", "parseInt",
  "decodeURI", "decodeURIComponent", "encodeURI", "encodeURIComponent", "escape", "unescape",
  "Object", "Function", "Boolean", "Symbol", "Error", "AggregateError", "EvalError", "RangeError",
  "ReferenceError", "SyntaxError", "TypeError", "URIError", "SuppressedError", "Number", "BigInt", "Math",
  "Date", "String", "RegExp", "Array", "Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array",
  "Uint16Array", "Int32Array", "Uint32Array", "Float16Array", "Float32Array", "Float64Array",
  "BigInt64Array", "BigUint64Array", "Map", "Set", "WeakMap", "WeakSet", "WeakRef",
  "FinalizationRegistry", "ArrayBuffer", "SharedArrayBuffer", "DataView", "Atomics", "JSON", "Promise",
  "Reflect", "Proxy", "Iterator", "DisposableStack", "AsyncDisposableStack", "Intl", "Temporal",
  // WebAssembly compiles bytes the plugin already holds; a module can call only the
  // functions the plugin hands it, so it reaches nothing the plugin's JavaScript cannot.
  "WebAssembly",
  // The worker itself (read-only facts)
  "self", "name", "location", "WorkerLocation", "navigator", "WorkerNavigator", "isSecureContext",
  "crossOriginIsolated", "origin", "console",
  // Events (EventTarget.prototype is left alone; every event target needs it)
  "EventTarget", "Event", "CustomEvent", "ErrorEvent", "MessageEvent", "PromiseRejectionEvent",
  "ProgressEvent", "AbortController", "AbortSignal", "DOMException",
  // Timers and scheduling (string timers are refused below)
  "setTimeout", "clearTimeout", "setInterval", "clearInterval", "queueMicrotask",
  "requestAnimationFrame", "cancelAnimationFrame", "structuredClone", "reportError",
  // Text, URLs, binary data
  "TextEncoder", "TextDecoder", "TextEncoderStream", "TextDecoderStream", "atob", "btoa", "URL",
  "URLSearchParams", "URLPattern", "Blob", "File", "FileReader", "FileReaderSync", "FormData",
  // Streams and compression
  "ReadableStream", "ReadableStreamDefaultReader", "ReadableStreamBYOBReader",
  "ReadableStreamDefaultController", "ReadableByteStreamController", "ReadableStreamBYOBRequest",
  "WritableStream", "WritableStreamDefaultWriter", "WritableStreamDefaultController", "TransformStream",
  "TransformStreamDefaultController", "ByteLengthQueuingStrategy", "CountQueuingStrategy",
  "CompressionStream", "DecompressionStream",
  // A Response without fetch is only a body reader (net.fetch answers with one)
  "Headers", "Request", "Response",
  // Crypto and timing
  "crypto", "Crypto", "SubtleCrypto", "CryptoKey", "performance", "Performance", "PerformanceEntry",
  "PerformanceMark", "PerformanceMeasure",
  // Drawing off screen (images for panels); these read no URLs
  "OffscreenCanvas", "OffscreenCanvasRenderingContext2D", "ImageBitmap", "ImageBitmapRenderingContext",
  "ImageData", "Path2D", "CanvasGradient", "CanvasPattern", "TextMetrics", "createImageBitmap",
  "DOMMatrix", "DOMMatrixReadOnly", "DOMPoint", "DOMPointReadOnly", "DOMRect", "DOMRectReadOnly", "DOMQuad",
]);

/** What a plugin's `navigator` keeps: facts about the browser, nothing that reaches anything. */
export const ALLOWED_NAVIGATOR: ReadonlySet<string> = new Set([
  "hardwareConcurrency", "language", "languages", "onLine", "platform", "userAgent", "appCodeName",
  "appName", "appVersion", "product",
]);

/**
 * Names a plugin must never have, checked after the allowlist has done its work (each is
 * also outside the allowlist). `fetch` comes back as a mediated function with "net".
 */
export const BLOCKED_GLOBALS = [
  "fetch", "XMLHttpRequest", "WebSocket", "WebSocketStream", "EventSource", "WebTransport",
  "importScripts", "postMessage", "close", "indexedDB", "IDBFactory", "caches", "CacheStorage",
  "Worker", "SharedWorker", "ServiceWorker", "BroadcastChannel", "MessageChannel", "MessagePort",
  "RTCPeerConnection", "RTCDataChannel", "FileSystemHandle", "FileSystemFileHandle",
  "FileSystemDirectoryHandle", "FileSystemSyncAccessHandle", "StorageManager", "StorageBucketManager",
  "Notification", "FontFace", "FontFaceSet", "fonts", "requestFileSystem", "webkitRequestFileSystem",
  "requestFileSystemSync", "webkitRequestFileSystemSync", "webkitResolveLocalFileSystemURL",
  "webkitResolveLocalFileSystemSyncURL", "TEMPORARY", "PERSISTENT", "onmessage",
] as const;

/** Navigator members a plugin must never have. */
export const BLOCKED_NAVIGATOR = ["storage", "storageBuckets", "serviceWorker", "locks", "usb", "hid", "serial", "bluetooth", "permissions", "gpu", "ml", "sendBeacon"] as const;

/** Remove `name` from `obj` and every prototype it inherits through. Returns whether any copy remains reachable. */
export function removeEverywhere(obj: object, name: string): boolean {
  let at: object | null = obj;
  const seen = new Set<object>();
  while (at && !seen.has(at)) {
    seen.add(at);
    if (Object.getOwnPropertyDescriptor(at, name)) removeOwn(at, name);
    at = Object.getPrototypeOf(at);
  }
  return reachable(obj, name);
}

function removeOwn(at: object, name: string): void {
  if (Reflect.deleteProperty(at, name)) return;
  // Not deletable: shadow it with nothing (fails for a non-configurable one; the caller checks).
  try {
    Object.defineProperty(at, name, { value: undefined, writable: false, configurable: false });
  } catch {
    try {
      (at as Record<string, unknown>)[name] = undefined;
    } catch {
      /* checked by the caller */
    }
  }
}

/** Some property by this name anywhere on `obj`'s prototype chain (no getter is run). */
export function hasAnywhere(obj: object, name: string): boolean {
  let at: object | null = obj;
  const seen = new Set<object>();
  while (at && !seen.has(at)) {
    seen.add(at);
    if (Object.getOwnPropertyDescriptor(at, name)) return true;
    at = Object.getPrototypeOf(at);
  }
  return false;
}

function reachable(obj: object, name: string): boolean {
  // Lane 15: a name that is gone from the whole chain is never read, so a deprecated getter
  // the lockdown removed (TEMPORARY, PERSISTENT, webkitRequestFileSystem ...) is not run and
  // the browser logs no deprecation warning for it.
  if (!hasAnywhere(obj, name)) return false;
  try {
    return (obj as Record<string, unknown>)[name] !== undefined;
  } catch {
    return false;
  }
}

/**
 * Delete every own string-keyed property of `obj` and of each prototype it inherits
 * through (up to, not including, any of `stops`) whose name is not in `allowed`. A
 * prototype keeps its `constructor`. Returns the names that are still reachable.
 */
export function stripToAllowlist(obj: object, allowed: ReadonlySet<string>, stops: readonly (object | null | undefined)[] = []): string[] {
  const stopAt = new Set<object>([Object.prototype, ...stops.filter((s): s is object => !!s)]);
  const removed = new Set<string>();
  let at: object | null = obj;
  const seen = new Set<object>();
  while (at && !stopAt.has(at) && !seen.has(at)) {
    seen.add(at);
    for (const name of Object.getOwnPropertyNames(at)) {
      if (allowed.has(name) || (at !== obj && name === "constructor")) continue;
      removeOwn(at, name);
      removed.add(name);
    }
    at = Object.getPrototypeOf(at);
  }
  return [...removed].filter((n) => reachable(obj, n));
}

function blocked(what: string): () => never {
  return function () {
    throw new EvalError(`${what} is not available to plugins: a plugin cannot turn text into code.`);
  };
}

export interface LockdownResult {
  /** Names that could not be removed (should be empty). */
  survivors: string[];
}

/**
 * Lock the worker's global scope down. `keep` names extra globals to leave (none in the app).
 * Afterwards only ALLOWED_GLOBALS remain: `typeof fetch === "undefined"`, `importScripts`,
 * `FontFace` and the file-system entry points are gone, `eval("1")` and `new Function("")`
 * throw, `setTimeout("code")` throws, `URL.createObjectURL` is gone, and `postMessage` is gone
 * from the global (the sandbox keeps its own bound copy, taken before).
 */
export function lockdown(scope: Record<string, unknown> & object, keep: string[] = []): LockdownResult {
  const survivors: string[] = [];
  const allowed = new Set([...ALLOWED_GLOBALS, ...keep]);
  const EventTargetCtor = scope.EventTarget as { prototype?: object } | undefined;
  survivors.push(...stripToAllowlist(scope, allowed, [EventTargetCtor?.prototype]));
  for (const name of BLOCKED_GLOBALS) if (!keep.includes(name) && removeEverywhere(scope, name)) if (!survivors.includes(name)) survivors.push(name);

  const nav = scope.navigator as object | undefined;
  if (nav) {
    for (const name of stripToAllowlist(nav, ALLOWED_NAVIGATOR)) survivors.push(`navigator.${name}`);
    for (const name of BLOCKED_NAVIGATOR) if (removeEverywhere(nav, name) && !survivors.includes(`navigator.${name}`)) survivors.push(`navigator.${name}`);
  }

  // Blob URLs the plugin made itself would be code it could import past the source check
  // (and they outlive the plugin); the sandbox took its own copy of createObjectURL first.
  const URLCtor = scope.URL as Record<string, unknown> | undefined;
  if (URLCtor) for (const name of ["createObjectURL", "revokeObjectURL"]) if (removeEverywhere(URLCtor, name)) survivors.push(`URL.${name}`);

  // Strings into code. The constructors stay recognisable (`f instanceof Function` holds)
  // but refuse to build anything.
  const NoFunction = blocked("Function");
  Object.defineProperty(NoFunction, "prototype", { value: Function.prototype });
  const protos = [
    Function.prototype,
    Object.getPrototypeOf(async function () {}),
    Object.getPrototypeOf(function* () {}),
    Object.getPrototypeOf(async function* () {}),
  ];
  for (const p of protos) {
    try {
      Object.defineProperty(p, "constructor", { value: p === Function.prototype ? NoFunction : blocked("Function"), writable: false, configurable: false });
    } catch {
      survivors.push("Function.prototype.constructor");
    }
  }
  for (const [name, value] of [
    ["Function", NoFunction],
    ["eval", blocked("eval")],
  ] as const) {
    try {
      Object.defineProperty(scope, name, { value, writable: false, configurable: false });
    } catch {
      survivors.push(name);
    }
  }
  // Timers take functions only.
  for (const name of ["setTimeout", "setInterval"]) {
    const original = scope[name] as ((...a: unknown[]) => unknown) | undefined;
    if (typeof original !== "function") continue;
    const safe = function (handler: unknown, ...rest: unknown[]) {
      if (typeof handler !== "function") throw new TypeError(`${name} takes a function in a plugin, not a string of code.`);
      return original.call(scope, handler, ...rest);
    };
    try {
      Object.defineProperty(scope, name, { value: safe, writable: false, configurable: false });
    } catch {
      survivors.push(name);
    }
  }
  return { survivors };
}
