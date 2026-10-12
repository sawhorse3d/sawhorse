// What a plugin's Web Worker cannot reach. Run by the sandbox before any plugin code loads.
//
// A dedicated worker already has no DOM, no window and no access to the app's memory; it
// talks to the app only by messages, and the app checks every message against the
// plugin's permissions. What is left to take away is the worker's own reach outside the
// app: the network (fetch, XHR, WebSocket, EventSource, WebTransport), storage the app
// does not mediate (IndexedDB, Cache Storage, the origin private file system), more
// workers, broadcast channels to the app's other pages, importScripts, and every way to
// turn a string into code (eval, Function and its async and generator cousins, string
// timers), which would get round the static check of the plugin's sources. Each is removed
// from the global object and from every prototype it is inherited through.

/** Globals a plugin never has. `fetch` comes back as a mediated function with the "net" permission. */
export const BLOCKED_GLOBALS = [
  "fetch",
  "XMLHttpRequest",
  "WebSocket",
  "WebSocketStream",
  "EventSource",
  "WebTransport",
  "importScripts",
  "indexedDB",
  "IDBFactory",
  "caches",
  "CacheStorage",
  "Worker",
  "SharedWorker",
  "ServiceWorker",
  "BroadcastChannel",
  "RTCPeerConnection",
  "RTCDataChannel",
  "FileSystemHandle",
  "FileSystemFileHandle",
  "FileSystemDirectoryHandle",
  "FileSystemSyncAccessHandle",
  "StorageManager",
  "Notification",
  "requestFileSystem",
  "webkitRequestFileSystem",
] as const;

/** Remove `name` from `obj` and every prototype it inherits through. Returns whether any copy remains reachable. */
export function removeEverywhere(obj: object, name: string): boolean {
  let at: object | null = obj;
  const seen = new Set<object>();
  while (at && !seen.has(at)) {
    seen.add(at);
    const d = Object.getOwnPropertyDescriptor(at, name);
    if (d) {
      if (!Reflect.deleteProperty(at, name)) {
        // Not deletable: shadow it with nothing.
        try {
          Object.defineProperty(at, name, { value: undefined, writable: false, configurable: false });
        } catch {
          /* checked below */
        }
      }
    }
    at = Object.getPrototypeOf(at);
  }
  try {
    return (obj as Record<string, unknown>)[name] !== undefined;
  } catch {
    return false;
  }
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
 * Lock the worker's global scope down. `keep` names globals to leave (none in the app).
 * Afterwards: `typeof fetch === "undefined"`, `importScripts` is gone, `eval("1")` and
 * `new Function("")` throw, `setTimeout("code")` throws, and `postMessage` is gone from the
 * global (the sandbox keeps its own bound copy, taken before).
 */
export function lockdown(scope: Record<string, unknown> & object, keep: string[] = []): LockdownResult {
  const survivors: string[] = [];
  for (const name of [...BLOCKED_GLOBALS, "postMessage"]) {
    if (keep.includes(name)) continue;
    if (removeEverywhere(scope, name)) survivors.push(name);
  }
  // The origin private file system and persistent-storage requests hang off navigator.
  const nav = scope.navigator as object | undefined;
  if (nav) for (const name of ["storage", "serviceWorker", "locks"]) if (removeEverywhere(nav, name)) survivors.push(`navigator.${name}`);

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
