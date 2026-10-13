/// <reference lib="webworker" />
// One plugin's sandbox: a dedicated Web Worker per plugin (ADR-0009). The app sends `init`
// with the plugin's modules; the sandbox locks the worker down (lockdown.ts), builds the API
// (packages/plugin-api/src/runtime.ts) where the "sawhorse" module finds it, links the
// plugin's modules as blob URLs (each import rewritten to the module it names, after the
// same checks the app made), imports the entry and says `ready`. From then on it relays
// messages: requests out, answers, callbacks, events and watchdog pings in.
//
// No DOM, no window, no network unless granted (and then only through the app), no
// importScripts, no eval: the global object is cut down to an allowlist (lockdown.ts), and
// the worker runs under the page's content security policy (it is started from a blob:
// URL, host.ts). A plugin that throws, loops or crashes takes down this worker and nothing
// else.
//
// Every message is one serialized frame, transferred (lane 16, wire.ts): what this worker
// sends is serialized once by the runtime and checked again by the app; what it receives is
// read with the same limits, and a frame it cannot read is answered (a pending call is
// rejected with a PluginApiError), never dropped.
import { lockdown } from "./lockdown";
import { createRuntime, type Runtime } from "../../packages/plugin-api/src/runtime.ts";
import { decodeHostFrame, encodePluginMessage, logText, peekFrame, type HostMessage, type LogLevel, type PluginMessage } from "../../packages/plugin-api/src/rpc.ts";
import { checkPackage } from "../../packages/plugin-api/src/source.ts";
import shimSource from "../../packages/plugin-api/sawhorse.js?raw";
import { formatLength, parseLength, UNIT_SYSTEMS, unitInfo, type UnitSystem } from "../lib/units";

const scope = self as unknown as DedicatedWorkerGlobalScope;
// Taken before lockdown removes them from the global object.
const postRaw = scope.postMessage.bind(scope);
const createObjectURL = URL.createObjectURL.bind(URL);
const consoleOriginal = { ...console };

/** Post one frame, transferring its buffer (no copy; what the app measures is what was sent). */
function post(frame: ArrayBuffer): void {
  postRaw(frame, [frame]);
}

/** Messages the sandbox itself sends (before the runtime exists, and its own errors). */
function send(msg: PluginMessage): void {
  try {
    post(encodePluginMessage(msg));
  } catch {
    try {
      post(encodePluginMessage({ t: "log", level: "error", text: `could not send a ${msg.t} message` }));
    } catch {
      /* nothing left to say it with */
    }
  }
}

let runtime: Runtime | null = null;
let started = false;

const asSystem = (s: string): UnitSystem => (UNIT_SYSTEMS.some((u) => u.id === s) ? (s as UnitSystem) : "frac-in");

/** Link the plugin's modules: each becomes a blob URL with its imports pointing at the others' URLs. */
function link(files: Record<string, string>, entry: string): string {
  const check = checkPackage(files, entry);
  if (check.problems.length) throw new Error(check.problems.join("\n"));
  const shimUrl = createObjectURL(new Blob([shimSource], { type: "text/javascript" }));
  const urls: Record<string, string> = {};
  for (const path of check.order) {
    let src = files[path];
    // Rewrite from the end so earlier positions stay valid.
    for (const l of [...check.links[path]].sort((a, b) => b.ref.start - a.ref.start)) {
      const target = l.path === null ? shimUrl : urls[l.path];
      src = src.slice(0, l.ref.start) + target + src.slice(l.ref.end);
    }
    urls[path] = createObjectURL(new Blob([`${src}\n//# sourceURL=plugin/${path}`], { type: "text/javascript" }));
  }
  return urls[entry];
}

async function start(init: Extract<HostMessage, { t: "init" }>): Promise<void> {
  if (started) return;
  started = true;
  const net = init.plugin.permissions.includes("net");
  const { survivors } = lockdown(scope as unknown as Record<string, unknown>);
  if (survivors.length) send({ t: "log", level: "warn", text: `sandbox: could not remove ${survivors.join(", ")}` });

  runtime = createRuntime({
    post,
    plugin: init.plugin,
    units: init.units,
    unitsImpl: {
      parse: (text, system) => parseLength(text, asSystem(system)),
      format: (m, system) => formatLength(m, asSystem(system)),
      short: (system) => unitInfo(asSystem(system)).short,
    },
  });
  const rt = runtime;
  Object.defineProperty(globalThis, Symbol.for("sawhorse.plugin-api"), { value: rt.api, writable: false, configurable: false, enumerable: false });
  if (net) {
    const api = rt.api as { net: { fetch: (url: string, init?: unknown) => Promise<unknown> } };
    Object.defineProperty(globalThis, "fetch", { value: (url: unknown, i?: unknown) => api.net.fetch(String(url instanceof URL ? url.href : url), i as never), writable: false, configurable: false });
  }
  // console.* goes to the plugin's developer console in the Plugins pane (and the browser's).
  for (const level of ["debug", "log", "info", "warn", "error"] as LogLevel[]) {
    (console as unknown as Record<string, unknown>)[level] = (...args: unknown[]) => {
      consoleOriginal[level]?.(`[${init.plugin.id}]`, ...args);
      rt.log(level, args);
    };
  }

  try {
    const entryUrl = link(init.files, init.entry);
    const mod = (await import(/* @vite-ignore */ entryUrl)) as { activate?: (api: unknown) => unknown };
    if (typeof mod.activate === "function") await mod.activate(rt.api);
    send({ t: "ready" });
  } catch (e) {
    send({ t: "fail", error: e instanceof Error ? `${e.name === "Error" ? "" : `${e.name}: `}${e.message}` : String(e) });
  }
}

scope.addEventListener("message", (ev: MessageEvent) => {
  // Once running, the runtime reads every frame (and answers what it cannot read).
  if (runtime) return runtime.receiveFrame(ev.data);
  let msg: HostMessage;
  try {
    msg = decodeHostFrame(ev.data);
  } catch (e) {
    const text = e instanceof Error ? e.message : String(e);
    // An init it cannot read fails the start, so the app's Enable never waits forever.
    if (peekFrame(ev.data)?.kind === "init" && !started) {
      started = true;
      send({ t: "fail", error: `the sandbox could not read the plugin's modules: ${text}` });
    } else send({ t: "log", level: "error", text: `sandbox: refused a message from the app: ${text}` });
    return;
  }
  if (msg.t === "init") {
    void start(msg);
    return;
  }
  if (msg.t === "ping") send({ t: "pong", n: msg.n });
});

// Errors the plugin did not catch: logged (the Plugins pane shows the last one); the
// plugin keeps running.
scope.addEventListener("error", (ev: ErrorEvent) => {
  ev.preventDefault();
  send({ t: "log", level: "error", text: `uncaught: ${ev.message}${ev.filename ? ` (${ev.filename.replace(/^blob:[^#]*/, "plugin")}:${ev.lineno})` : ""}` });
});
scope.addEventListener("unhandledrejection", (ev: PromiseRejectionEvent) => {
  ev.preventDefault();
  send({ t: "log", level: "error", text: `unhandled rejection: ${logText([ev.reason])}` });
});

