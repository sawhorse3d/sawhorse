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
import type { Permission } from "../../packages/plugin-api/src/manifest.ts";
import { decodePluginMessage, PROTOCOL, ProtocolError, transferables, type Data, type HostMessage, type LogLevel, type PluginMessage } from "../../packages/plugin-api/src/rpc.ts";
import { moduleFiles, type PluginPackage } from "./loader";

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
export const EVENTS: Record<string, Permission> = {
  selectionChanged: "model.read",
  documentChanged: "model.read",
  documentOpened: "model.read",
  toolChanged: "ui",
};

/** Why a request was refused, for the plugin (`error.code`). */
export type RefusalCode = "permission" | "unknown" | "invalid" | "refused" | "busy" | "stopped";

export class PluginRefusal extends Error {
  readonly code: RefusalCode;
  constructor(message: string, code: RefusalCode = "refused") {
    super(message);
    this.name = "PluginRefusal";
    this.code = code;
  }
}

export interface Channel {
  post(msg: HostMessage, transfer?: Transferable[]): void;
  onMessage(cb: (raw: unknown) => void): void;
  /** The worker itself failed (its script would not load, or an error escaped the sandbox). */
  onError(cb: (message: string) => void): void;
  terminate(): void;
}

/** A plugin's real home: its own module Web Worker running the sandbox. */
export function workerChannel(name: string): Channel {
  const w = new Worker(new URL("./sandbox.worker.ts", import.meta.url), { type: "module", name });
  return {
    post: (msg, transfer = []) => w.postMessage(msg, transfer),
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
}

export const DEFAULT_HOST_OPTIONS: HostOptions = { pingMs: 1000, budgetMs: 5000, killMs: 15000, callBudgetMs: 10000, maxPending: 256, maxLog: 500 };

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

  constructor(pkg: PluginPackage, channel: Channel, delegate: HostDelegate, opts: Partial<HostOptions> = {}) {
    this.pkg = pkg;
    this.id = pkg.manifest.id;
    this.channel = channel;
    this.delegate = delegate;
    this.opts = { ...DEFAULT_HOST_OPTIONS, ...opts };
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

  /** Send the modules and wait for the entry to run. Rejects with the reason it did not. */
  start(units: string): Promise<void> {
    const m = this.pkg.manifest;
    this.lastPong = Date.now();
    this.startWatchdog();
    const done = new Promise<void>((resolve, reject) => (this.ready = { resolve, reject }));
    this.post({ t: "init", protocol: PROTOCOL, plugin: { id: m.id, name: m.name, version: m.version, permissions: [...m.permissions] }, files: moduleFiles(this.pkg), entry: m.entry, units });
    return done;
  }

  /** Ask the plugin to run one of its callbacks (a command, a panel event, a tool event). */
  call(fn: string, args: Data[], label = fn): Promise<Data> {
    if (!this.alive) return Promise.reject(new PluginRefusal(`${this.name} is not running`, "stopped"));
    const id = this.nextCall++;
    return new Promise((resolve, reject) => {
      this.calls.set(id, { fn, label, started: Date.now(), reported: false, resolve, reject });
      this.post({ t: "call", id, fn, args });
    });
  }

  /** Callbacks still running. */
  busyCalls(): string[] {
    return [...this.calls.values()].map((c) => c.label);
  }

  /** Deliver an event the plugin listens to (and is allowed to). */
  emit(name: string, payload: Data): void {
    if (!this.alive || !this.subscriptions.has(name)) return;
    this.post({ t: "evt", name, payload });
  }

  /** End the plugin: its worker is terminated, its callbacks fail. */
  stop(reason: string | null = null, status: HostStatus = "stopped"): void {
    if (!this.alive && this.status !== "starting") return;
    this.status = status;
    if (reason) this.lastError = reason;
    this.stopWatchdog();
    this.channel.terminate();
    for (const c of this.calls.values()) c.reject(new PluginRefusal(reason ?? `${this.name} stopped`, "stopped"));
    this.calls.clear();
    this.ready?.reject(new Error(reason ?? `${this.name} stopped before it finished loading`));
    this.ready = null;
    this.delegate.changed(this);
  }

  // ---- internals ----------------------------------------------------------------------------

  private post(msg: HostMessage): void {
    try {
      this.channel.post(msg, transferables(msg));
    } catch (e) {
      this.log("error", `could not send to the plugin: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  private crash(text: string): void {
    if (!this.alive) return;
    this.log("error", text);
    this.stop(text, "crashed");
    this.delegate.report(this, `${this.name} crashed: ${text}. It is stopped; Reload starts it again.`, "error", "reload");
  }

  log(level: LogLevel, text: string): void {
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

  private onMessage(raw: unknown): void {
    let msg: PluginMessage;
    try {
      msg = decodePluginMessage(raw);
    } catch (e) {
      this.log("error", `refused a message: ${e instanceof ProtocolError ? e.message : String(e)}`);
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
    const refuse = (error: string, code: RefusalCode) => this.post({ t: "res", id, ok: false, error, code });
    if (!(method in METHODS)) return refuse(`there is no ${method} in plugin API 1`, "unknown");
    let need = METHODS[method];
    if (method === "events.subscribe") {
      const name = (params as { name?: unknown } | null)?.name;
      if (typeof name !== "string" || !(name in EVENTS)) return refuse(`there is no event ${JSON.stringify(name)} (events: ${Object.keys(EVENTS).join(", ")})`, "invalid");
      need = EVENTS[name];
    }
    if (need && !this.has(need)) {
      this.log("warn", `refused ${method}: the manifest does not ask for "${need}"`);
      return refuse(`${this.name} may not use ${method}: plugin.json does not ask for the "${need}" permission`, "permission");
    }
    if (this.inFlight >= this.opts.maxPending) return refuse(`too many requests at once (more than ${this.opts.maxPending})`, "busy");
    this.inFlight++;
    try {
      let value: Data;
      if (method === "events.subscribe") {
        this.subscriptions.add((params as { name: string }).name);
        value = null;
      } else value = await this.delegate.handle(this, method, params);
      if (this.alive) this.post({ t: "res", id, ok: true, value });
    } catch (e) {
      const code: RefusalCode = e instanceof PluginRefusal ? e.code : "refused";
      if (this.alive) refuse(e instanceof Error ? e.message : String(e), code);
    } finally {
      this.inFlight--;
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
