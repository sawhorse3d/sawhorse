// The app's side of one running plugin: its worker (behind a Channel, so tests can stand
// in a fake), the request/answer bookkeeping, the permission check on every request, and
// the watchdog.
//
// Permissions are enforced here, never trusted from the plugin: a request names a method,
// the method needs one permission (METHODS), and the plugin's manifest either grants it or
// the request is refused with code "permission" before anything else looks at it.
//
// The watchdog pings the worker every second. A worker that has not answered for
// `budgetMs` is reported unresponsive (the Plugins pane and the status line say so, with
// Stop); one that has not answered for `killMs` is stopped. The worker is a separate
// thread, so a plugin stuck in a loop never freezes the app; a crash ends only its worker.
//
// Lane 16, the message path: every message is one serialized frame (packages/plugin-api
// src/wire.ts). A frame's size is checked against its kind's budget (4 MiB, more only for
// file.save, file.open and net.fetch) before anything in it is read, and its values are read
// with every limit applied before anything is allocated for them; a refused request is
// answered with a typed refusal ("too_large", "invalid", "busy"), never dropped. Requests are
// rate-limited (a token bucket per plugin, a cap on batches a second and on work in flight
// per method), and the time the app spends on a plugin's messages is measured: a plugin that
// keeps the app busy is reported and stopped like an unresponsive one.
import type { Permission } from "../../packages/plugin-api/src/manifest.ts";
import { decodePluginFrame, encodeHostMessage, peekFrame, PROTOCOL, WireError, type Data, type HostMessage, type LogLevel, type PluginMessage } from "../../packages/plugin-api/src/rpc.ts";
import { moduleFiles, type PluginPackage } from "./loader";
import sandboxScriptUrl from "./sandbox.worker.ts?worker&url";

const own = (o: object, k: string) => Object.prototype.hasOwnProperty.call(o, k);

/** What each request method needs. */
export const METHODS: Record<string, Permission | null> = {
  "model.document": "model.read",
  "model.stats": "model.read",
  "model.solids": "model.read",
  "model.solid": "model.read",
  "model.faces": "model.read",
  "model.nodes": "model.read",
  "model.definitions": "model.read",
  "model.tree": "model.read",
  "model.selection": "model.read",
  "model.select": "model.read",
  "model.bounds": "model.read",
  "model.materials": "model.read",
  "model.cutList": "model.read",
  "model.batch": "model.write",
  "ui.registerCommand": "ui",
  "ui.registerPanel": "ui",
  "ui.updatePanel": "ui",
  "ui.registerTool": "ui",
  "ui.toolView": "ui",
  "ui.activateTool": "ui",
  "ui.notify": "ui",
  "ui.prompt": "ui",
  "ui.confirm": "ui",
  // Checked per event name (EVENTS) instead.
  "events.subscribe": null,
  "storage.get": "storage",
  "storage.set": "storage",
  "storage.delete": "storage",
  "storage.keys": "storage",
  "file.save": "file",
  "file.open": "file",
  "net.fetch": "net",
};

/** The events a plugin may listen to, and what each needs. */
export const EVENTS: Record<string, Permission | null> = {
  selectionChanged: "model.read",
  documentChanged: "model.read",
  documentOpened: "model.read",
  toolChanged: "ui",
  // Every plugin follows the display units (sawhorse.units); listening needs nothing.
  unitsChanged: null,
};

/** Why a request was refused, for the plugin (`error.code`). */
export type RefusalCode = "permission" | "unknown" | "invalid" | "refused" | "busy" | "stopped" | "too_large";

export class PluginRefusal extends Error {
  readonly code: RefusalCode;
  constructor(message: string, code: RefusalCode = "refused") {
    super(message);
    this.name = "PluginRefusal";
    this.code = code;
  }
}

export interface Channel {
  /** Post one serialized frame; its buffer is transferred (the sender no longer has it). */
  post(frame: ArrayBuffer): void;
  onMessage(cb: (raw: unknown) => void): void;
  /** The worker itself failed (its script would not load, or an error escaped the sandbox). */
  onError(cb: (message: string) => void): void;
  terminate(): void;
}

/**
 * The source of the blob: module that starts a sandbox: one static import of the sandbox
 * script at its absolute URL. A worker started from a blob: URL runs under the policy of the
 * page that made it (CSP 3, "local scheme" inheritance); one started from its own URL would
 * run under that response's headers, which a static host cannot set. So the page's content
 * security policy (vite.config.ts in the browser build, tauri.conf.json on the desktop)
 * also governs every plugin: no scripts or fonts from anywhere but the app itself.
 */
export function sandboxBootstrap(sandboxUrl: string, base: string): string {
  return `import ${JSON.stringify(new URL(sandboxUrl, base).href)};\n`;
}

/** A plugin's real home: its own module Web Worker running the sandbox, under the page's policy. */
export function workerChannel(name: string): Channel {
  const boot = URL.createObjectURL(new Blob([sandboxBootstrap(sandboxScriptUrl, document.baseURI)], { type: "text/javascript" }));
  const w = new Worker(boot, { type: "module", name });
  // The worker has fetched (or is fetching) its script; the URL is not needed again.
  setTimeout(() => URL.revokeObjectURL(boot), 60_000);
  return {
    post: (frame) => w.postMessage(frame, [frame]),
    onMessage: (cb) => w.addEventListener("message", (e: MessageEvent) => cb(e.data)),
    onError: (cb) => {
      w.addEventListener("error", (e: ErrorEvent) => {
        e.preventDefault();
        cb(e.message || "the plugin's worker failed to start");
      });
      w.addEventListener("messageerror", () => cb("a message from the plugin could not be read"));
    },
    terminate: () => w.terminate(),
  };
}

export type HostStatus = "starting" | "running" | "unresponsive" | "crashed" | "stopped" | "failed";

export interface LogLine {
  at: number;
  level: LogLevel;
  text: string;
}

export interface HostDelegate {
  /** Run a request the permission check let through. Throw a PluginRefusal to refuse it. */
  handle(host: PluginHost, method: string, params: Data): Promise<Data>;
  /** Status, last error or the log changed. */
  changed(host: PluginHost): void;
  /** Something worth telling the person (unresponsive, crashed, slow). */
  report(host: PluginHost, text: string, kind: "warning" | "error", action?: "stop" | "reload"): void;
}

export interface HostOptions {
  /** Ping interval. */
  pingMs: number;
  /** No answer for this long: reported unresponsive. */
  budgetMs: number;
  /** No answer for this long: stopped. */
  killMs: number;
  /** A callback the plugin is still running after this long (while not waiting on the app): reported slow. */
  callBudgetMs: number;
  /** Most requests waiting for an answer at once. */
  maxPending: number;
  /** Lines kept for the developer console. */
  maxLog: number;
  /** Messages a second a plugin may send (a token bucket refilled at this rate) ... */
  ratePerSec: number;
  /** ... holding at most this many at once. */
  burst: number;
  /** Batches a second a plugin may apply. */
  batchesPerSec: number;
  /** App time (ms) a plugin's messages may take in one second before that second counts against it ... */
  cpuMsPerSec: number;
  /** ... and how many such seconds in a row stop it. */
  cpuStrikes: number;
}

export const DEFAULT_HOST_OPTIONS: HostOptions = { pingMs: 1000, budgetMs: 5000, killMs: 15000, callBudgetMs: 10000, maxPending: 64, maxLog: 500, ratePerSec: 300, burst: 100, batchesPerSec: 10, cpuMsPerSec: 250, cpuStrikes: 5 };

/** Requests of one method a plugin may have waiting at once (the rest: maxPending). */
export const IN_FLIGHT: Record<string, number> = {
  "model.batch": 4,
  "net.fetch": 4,
  "file.save": 1,
  "file.open": 1,
  // One dialog at a time per plugin (prompt and confirm share it).
  dialog: 1,
};

/** Longest line kept in a plugin's console, and longest error the Plugins pane shows. */
export const LOG_TEXT_MAX = 2000;
const clip = (t: string, max = LOG_TEXT_MAX) => (t.length > max ? `${t.slice(0, max)}… (${t.length - max} more characters)` : t);

interface PendingCall {
  fn: string;
  label: string;
  started: number;
  reported: boolean;
  resolve: (v: Data) => void;
  reject: (e: Error) => void;
}

export class PluginHost {
  readonly pkg: PluginPackage;
  readonly id: string;
  status: HostStatus = "starting";
  lastError: string | null = null;
  readonly logs: LogLine[] = [];
  /** Events the plugin listens to. */
  readonly subscriptions = new Set<string>();
  private channel: Channel;
  private delegate: HostDelegate;
  private opts: HostOptions;
  private nextCall = 1;
  private calls = new Map<number, PendingCall>();
  /** Requests from the plugin the app is still answering. */
  private inFlight = 0;
  private pingN = 0;
  private lastPong = 0;
  private pingTimer: ReturnType<typeof setInterval> | null = null;
  private ready: { resolve: () => void; reject: (e: Error) => void } | null = null;
  private logWindow = { start: 0, count: 0, dropped: 0 };
  /** The rate limit: tokens left and when they were last refilled. */
  private bucket = { tokens: 0, at: 0 };
  /** Batches applied in the current second. */
  private batchWindow = { start: 0, count: 0 };
  /** Requests waiting, by method (and "dialog"). */
  private flying = new Map<string, number>();
  /** Time spent on this plugin's messages in the current second, and seconds over budget in a row. */
  private cpu = { start: 0, ms: 0, strikes: 0 };
  /** Messages refused for the rate in the current second. */
  private refusedWindow = { start: 0, count: 0 };
  /** Aborted when the plugin stops: work it asked for and that has not run yet is dropped. */
  private abort = new AbortController();

  constructor(pkg: PluginPackage, channel: Channel, delegate: HostDelegate, opts: Partial<HostOptions> = {}) {
    this.pkg = pkg;
    this.id = pkg.manifest.id;
    this.channel = channel;
    this.delegate = delegate;
    this.opts = { ...DEFAULT_HOST_OPTIONS, ...opts };
    this.bucket = { tokens: this.opts.burst, at: Date.now() };
    channel.onMessage((raw) => this.onMessage(raw));
    channel.onError((message) => this.crash(`the plugin's worker failed: ${message}`));
  }

  get name(): string {
    return this.pkg.manifest.name;
  }

  get alive(): boolean {
    return this.status === "starting" || this.status === "running" || this.status === "unresponsive";
  }

  has(p: Permission): boolean {
    return this.pkg.manifest.permissions.includes(p);
  }

  /** Aborted when the plugin stops (queued batches and dialogs check it). */
  get signal(): AbortSignal {
    return this.abort.signal;
  }

  /** Send the modules and wait for the entry to run. Rejects with the reason it did not. */
  start(units: string): Promise<void> {
    const m = this.pkg.manifest;
    this.lastPong = Date.now();
    this.startWatchdog();
    const done = new Promise<void>((resolve, reject) => (this.ready = { resolve, reject }));
    const err = this.post({ t: "init", protocol: PROTOCOL, plugin: { id: m.id, name: m.name, version: m.version, permissions: [...m.permissions] }, files: moduleFiles(this.pkg), entry: m.entry, units });
    if (err) this.stop(`${this.name} could not start: its modules could not be sent (${err.message})`, "failed");
    return done;
  }

  /** Ask the plugin to run one of its callbacks (a command, a panel event, a tool event). */
  call(fn: string, args: Data[], label = fn): Promise<Data> {
    if (!this.alive) return Promise.reject(new PluginRefusal(`${this.name} is not running`, "stopped"));
    const id = this.nextCall++;
    return new Promise((resolve, reject) => {
      this.calls.set(id, { fn, label, started: Date.now(), reported: false, resolve, reject });
      const err = this.post({ t: "call", id, fn, args });
      if (err) {
        this.calls.delete(id);
        reject(new PluginRefusal(`${label}: the call could not be sent (${err.message})`, err.code));
      }
    });
  }

  /** Callbacks still running. */
  busyCalls(): string[] {
    return [...this.calls.values()].map((c) => c.label);
  }

  /** Deliver an event the plugin listens to (and is allowed to). */
  emit(name: string, payload: Data): void {
    // The units always go (sawhorse.units follows them); the rest only when listened to.
    if (!this.alive || (name !== "unitsChanged" && !this.subscriptions.has(name))) return;
    const err = this.post({ t: "evt", name, payload });
    if (err) this.log("warn", `a ${name} event was not delivered: ${err.message}`);
  }

  /** End the plugin: its worker is terminated, its callbacks fail. */
  stop(reason: string | null = null, status: HostStatus = "stopped"): void {
    if (!this.alive && this.status !== "starting") return;
    this.status = status;
    if (reason) this.lastError = clip(reason);
    this.abort.abort();
    this.stopWatchdog();
    this.channel.terminate();
    for (const c of this.calls.values()) c.reject(new PluginRefusal(reason ?? `${this.name} stopped`, "stopped"));
    this.calls.clear();
    this.ready?.reject(new Error(reason ?? `${this.name} stopped before it finished loading`));
    this.ready = null;
    this.delegate.changed(this);
  }

  // ---- internals ----------------------------------------------------------------------------

  /**
   * Serialize and post a message. A message that cannot be serialized (over its budget, not
   * plain data) is not sent and the error comes back: an answer is replaced by a refusal with
   * the same code, so the plugin's call settles; the other callers decide.
   */
  private post(msg: HostMessage): WireError | null {
    let frame: ArrayBuffer;
    try {
      frame = encodeHostMessage(msg);
    } catch (e) {
      const err = e instanceof WireError ? e : new WireError(e instanceof Error ? e.message : String(e), "invalid");
      if (msg.t === "res" && msg.ok) {
        this.log("warn", `the answer to ${msg.method ?? "a request"} was not sent: ${err.message}`);
        return this.post({ t: "res", id: msg.id, ok: false, error: `${msg.method ?? "the request"}: the answer is too large to send (${err.message})`, code: err.code, method: msg.method });
      }
      return err;
    }
    try {
      this.channel.post(frame);
    } catch (e) {
      this.log("error", `could not send to the plugin: ${e instanceof Error ? e.message : String(e)}`);
    }
    return null;
  }

  private crash(text: string): void {
    if (!this.alive) return;
    this.log("error", text);
    this.stop(text, "crashed");
    this.delegate.report(this, `${this.name} crashed: ${text}. It is stopped; Reload starts it again.`, "error", "reload");
  }

  log(level: LogLevel, text: string): void {
    // Lines are cut here, whoever wrote them (the plugin's own lines are cut to 4,000 on the
    // wire; the app's lines quote what a plugin sent).
    text = clip(text);
    const now = Date.now();
    // At most 100 lines a second reach the console; the rest are counted.
    if (now - this.logWindow.start > 1000) {
      if (this.logWindow.dropped) this.logs.push({ at: now, level: "warn", text: `(${this.logWindow.dropped} log lines dropped: more than 100 a second)` });
      this.logWindow = { start: now, count: 0, dropped: 0 };
    }
    if (++this.logWindow.count > 100) {
      this.logWindow.dropped++;
      return;
    }
    this.logs.push({ at: now, level, text });
    if (this.logs.length > this.opts.maxLog) this.logs.splice(0, this.logs.length - this.opts.maxLog);
    if (level === "error") this.lastError = text;
    this.delegate.changed(this);
  }

  /** Take a token from the rate bucket; false: over the rate. */
  private take(): boolean {
    const now = Date.now();
    const b = this.bucket;
    b.tokens = Math.min(this.opts.burst, b.tokens + ((now - b.at) * this.opts.ratePerSec) / 1000);
    b.at = now;
    if (b.tokens < 1) return false;
    b.tokens -= 1;
    return true;
  }

  /** Count a message refused for the rate; ten seconds' worth in one second stops the plugin. */
  private flooding(): boolean {
    const now = Date.now();
    if (now - this.refusedWindow.start > 1000) this.refusedWindow = { start: now, count: 0 };
    if (++this.refusedWindow.count === 1) this.log("warn", `slowed down: more than ${this.opts.ratePerSec} messages a second (the rest are refused with "busy")`);
    if (this.refusedWindow.count > this.opts.ratePerSec * 10) {
      const text = `${this.name} stopped: it sent more than ${this.opts.ratePerSec * 10} messages in a second. Reload starts it again.`;
      this.log("error", text);
      this.stop(text, "stopped");
      this.delegate.report(this, text, "error", "reload");
      return true;
    }
    return false;
  }

  /** Time spent on this plugin's messages: a plugin that keeps the app busy is stopped. */
  private charge(ms: number): void {
    const now = Date.now();
    if (now - this.cpu.start >= 1000) {
      // A second over budget is a strike; a second under it clears them.
      const over = this.cpu.ms > this.opts.cpuMsPerSec;
      this.cpu = { start: now, ms: 0, strikes: over ? this.cpu.strikes + 1 : 0 };
      if (over && this.cpu.strikes >= this.opts.cpuStrikes && this.alive) {
        const text = `${this.name} stopped: its requests kept the app busy for ${this.opts.cpuStrikes} s. Reload starts it again.`;
        this.log("error", text);
        this.stop(text, "stopped");
        this.delegate.report(this, text, "error", "reload");
        return;
      }
    }
    this.cpu.ms += ms;
  }

  private onMessage(raw: unknown): void {
    if (!this.alive) return;
    const t0 = performance.now();
    try {
      this.receive(raw);
    } finally {
      this.charge(performance.now() - t0);
    }
  }

  private receive(raw: unknown): void {
    // The header first: its size and kind decide the budget before any value is read.
    const head = peekFrame(raw);
    if (head && head.kind !== "pong" && head.kind !== "done" && !this.take()) {
      if (this.flooding()) return;
      if (head.kind === "req") this.post({ t: "res", id: head.id, ok: false, error: `too many requests: at most ${this.opts.ratePerSec} a second`, code: "busy", method: head.name });
      else if (head.kind === "log") this.logWindow.dropped++;
      return;
    }
    let msg: PluginMessage;
    try {
      msg = decodePluginFrame(raw);
    } catch (e) {
      const text = e instanceof Error ? e.message : String(e);
      const code = e instanceof WireError ? e.code : "invalid";
      this.log("error", `refused a message: ${text}`);
      // A request or an answer that cannot be read is answered, never dropped.
      if (head?.kind === "req") this.post({ t: "res", id: head.id, ok: false, error: `${head.name}: the request could not be read (${text})`, code, method: head.name });
      else if (head?.kind === "done") {
        const c = this.calls.get(head.id);
        if (c) {
          this.calls.delete(head.id);
          c.reject(new PluginRefusal(`the plugin's answer could not be read (${text})`, code));
        }
      }
      return;
    }
    switch (msg.t) {
      case "ready":
        if (this.status === "starting") this.status = "running";
        this.ready?.resolve();
        this.ready = null;
        this.delegate.changed(this);
        return;
      case "fail":
        this.log("error", msg.error);
        this.stop(`${this.name} could not start: ${msg.error}`, "failed");
        return;
      case "pong":
        this.lastPong = Date.now();
        if (this.status === "unresponsive") {
          this.status = this.ready ? "starting" : "running";
          this.log("info", "responding again");
        }
        return;
      case "log":
        this.log(msg.level, msg.text);
        return;
      case "done": {
        const c = this.calls.get(msg.id);
        if (!c) return;
        this.calls.delete(msg.id);
        if (msg.ok) c.resolve(msg.value);
        else c.reject(new Error(msg.error));
        return;
      }
      case "req":
        void this.request(msg.id, msg.method, msg.params);
        return;
    }
  }

  private async request(id: number, method: string, params: Data): Promise<void> {
    const refuse = (error: string, code: RefusalCode) => void this.post({ t: "res", id, ok: false, error, code, method });
    if (!own(METHODS, method)) return refuse(`there is no ${method} in plugin API 1`, "unknown");
    let need = METHODS[method];
    if (method === "events.subscribe") {
      const name = (params as { name?: unknown } | null)?.name;
      if (typeof name !== "string" || !own(EVENTS, name)) return refuse(`there is no event ${JSON.stringify(typeof name === "string" ? name.slice(0, 40) : name)} (events: ${Object.keys(EVENTS).join(", ")})`, "invalid");
      need = EVENTS[name];
    }
    if (need && !this.has(need)) {
      this.log("warn", `refused ${method}: the manifest does not ask for "${need}"`);
      return refuse(`${this.name} may not use ${method}: plugin.json does not ask for the "${need}" permission`, "permission");
    }
    if (this.inFlight >= this.opts.maxPending) return refuse(`too many requests at once (more than ${this.opts.maxPending})`, "busy");
    // Work in flight per method: batches, fetches, files, one dialog at a time.
    const slot = method === "ui.prompt" || method === "ui.confirm" ? "dialog" : method;
    const cap = IN_FLIGHT[slot];
    if (cap !== undefined && (this.flying.get(slot) ?? 0) >= cap) return refuse(`${method}: ${cap === 1 ? "one is already waiting" : `${cap} are already waiting`}; wait for ${cap === 1 ? "it" : "them"} first`, "busy");
    if (method === "model.batch") {
      const now = Date.now();
      if (now - this.batchWindow.start >= 1000) this.batchWindow = { start: now, count: 0 };
      if (++this.batchWindow.count > this.opts.batchesPerSec) return refuse(`too many batches: at most ${this.opts.batchesPerSec} a second`, "busy");
    }
    this.inFlight++;
    if (cap !== undefined) this.flying.set(slot, (this.flying.get(slot) ?? 0) + 1);
    try {
      let value: Data;
      if (method === "events.subscribe") {
        this.subscriptions.add((params as { name: string }).name);
        value = null;
      } else value = await this.delegate.handle(this, method, params);
      if (this.alive) this.post({ t: "res", id, ok: true, value, method });
    } catch (e) {
      const code: RefusalCode = e instanceof PluginRefusal ? e.code : "refused";
      if (this.alive) refuse(e instanceof Error ? e.message : String(e), code);
    } finally {
      this.inFlight--;
      if (cap !== undefined) this.flying.set(slot, (this.flying.get(slot) ?? 1) - 1);
    }
  }

  private startWatchdog(): void {
    this.stopWatchdog();
    this.pingTimer = setInterval(() => this.tick(), this.opts.pingMs);
  }

  private stopWatchdog(): void {
    if (this.pingTimer !== null) clearInterval(this.pingTimer);
    this.pingTimer = null;
  }

  private tick(): void {
    if (!this.alive) return this.stopWatchdog();
    const now = Date.now();
    const silent = now - this.lastPong;
    const running = this.busyCalls();
    const doing = running.length ? ` (running ${running.map((r) => `"${r}"`).join(", ")})` : this.status === "starting" ? " (while loading)" : "";
    if (silent >= this.opts.killMs) {
      const text = `${this.name} stopped: it did not respond for ${Math.round(silent / 1000)} s${doing}, so it was ended. Reload starts it again.`;
      this.log("error", text);
      this.stop(text, "stopped");
      this.delegate.report(this, text, "error", "reload");
      return;
    }
    if (silent >= this.opts.budgetMs && this.status !== "unresponsive") {
      this.status = "unresponsive";
      const text = `${this.name} is not responding${doing}. It is stopped in ${Math.round((this.opts.killMs - silent) / 1000)} s, or stop it now.`;
      this.log("warn", text);
      this.delegate.report(this, text, "warning", "stop");
    }
    // A callback running past its budget while the plugin is not waiting on the app.
    if (this.inFlight === 0)
      for (const c of this.calls.values())
        if (!c.reported && now - c.started >= this.opts.callBudgetMs) {
          c.reported = true;
          this.log("warn", `"${c.label}" has been running for more than ${Math.round(this.opts.callBudgetMs / 1000)} s`);
        }
    this.pingN++;
    this.post({ t: "ping", n: this.pingN });
  }
}
