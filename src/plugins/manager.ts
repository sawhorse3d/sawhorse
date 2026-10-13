// The plugin registry and everything the app does for plugins: which plugins exist (bundled,
// installed, unpacked, loaded for the session), which are enabled and with what grant, one
// PluginHost per running plugin, what each has added (commands, panels, tools), and the
// answers to their requests (the model, the window, storage, files, the network). React
// reads it through `subscribe` / `version` (usePlugins.ts); the app gives it an AppBridge.
import { hostAllowed, PERMISSION_TEXT, type Manifest, type Permission, type ToolContribution } from "../../packages/plugin-api/src/manifest.ts";
import type { Data } from "../../packages/plugin-api/src/rpc.ts";
import type { KernelClient } from "../kernel/client";
import type { CutList, Entity, OpOutcome, Rounding, SceneData } from "../kernel/types";
import { NeedsUserActivation, type FileType, type PickedFile } from "../io/files";
import { scopeEntity } from "../lib/scope";
import type { UnitSystem } from "../lib/units";
import { bundledPlugins } from "./bundled";
import { DEFAULT_HOST_OPTIONS, PluginHost, PluginRefusal, workerChannel, type Channel, type HostOptions, type HostStatus, type LogLine } from "./host";
import { preparePackage, type Prepared } from "./checker";
import { loadPackage, type PluginPackage, type PluginSource } from "./loader";
import { boundsOf, cutListOf, definitionsOf, entitiesOfTargets, nodesOf, selectionOf, solidOf, solidsOf, statsOf, treeOf } from "./model";
import { checkBody, patchFields, valuesOf, type FieldValue, type PanelField } from "./panel";
import type { PluginPlatform } from "./platform";
import { AppStore, DocStore, grantCovers, grantMatches, loadGrants, packageDigest, saveGrants, type Grant } from "./store";
import { redactToolEvent } from "./toolEvent";
import { checkToolView, type PluginToolView } from "./toolView";
import { readPluginZip } from "./zip";

/** Ops a plugin batch may not hold: history, nesting batches, navigation, whole-document replacement. */
export const FORBIDDEN_OPS = new Set(["undo", "redo", "batch", "set_context", "leave_context", "restore"]);
/** Most ops one batch may hold (crates/kernel/src/refbatch.rs MAX_OPS; lane 16: was 10,000). */
export const MAX_BATCH_OPS = 2_000;

export type DialogRequest =
  | { kind: "prompt"; plugin: string; title: string; label: string; value: string; ok: string }
  | { kind: "confirm"; plugin: string; title: string; text: string; ok: string }
  /** Lane 17: the app's own question before a plugin's file goes to the browser's downloads. */
  | { kind: "download"; plugin: string; name: string; bytes: number };

/**
 * Lane 17: file names a plugin may not save, by their last extension: programs, scripts,
 * shortcuts and pages a double-click would run (Windows, macOS, Linux and the browser).
 * A plugin's report is .md, .csv, .txt, .json, .svg, .dxf, .pdf ...; nothing it needs ends
 * in one of these.
 */
export const BLOCKED_SAVE_EXTENSIONS = new Set([
  // Windows programs, installers, scripts and shortcuts
  ".exe", ".com", ".scr", ".pif", ".cpl", ".msi", ".msp", ".msc", ".bat", ".cmd", ".ps1", ".psm1", ".psd1", ".ps1xml", ".vbs", ".vbe", ".js", ".jse", ".mjs", ".cjs", ".wsf", ".wsh", ".wsc", ".hta", ".lnk", ".url", ".scf", ".reg", ".inf", ".dll", ".sys", ".appx", ".appxbundle", ".msix", ".msixbundle", ".appref-ms", ".application", ".gadget", ".chm", ".settingcontent-ms",
  // macOS and Linux programs, packages and launchers
  ".app", ".command", ".pkg", ".dmg", ".sh", ".bash", ".zsh", ".csh", ".ksh", ".fish", ".run", ".desktop", ".deb", ".rpm", ".appimage", ".so", ".dylib", ".workflow", ".terminal",
  // Cross-platform runtimes and pages that run script when opened
  ".jar", ".jnlp", ".py", ".pyw", ".pl", ".rb", ".php", ".apk", ".html", ".htm", ".xhtml", ".shtml", ".mht", ".mhtml", ".xht",
]);

/** Lane 16: the network answer a plugin may receive, and how much it may download a minute. */
export const NET_LIMITS = { answer: 10 * 1024 * 1024, perMinute: 50 * 1024 * 1024, seconds: 30 };

/** Lane 16: what prompt and confirm take (longer text is refused, not cut). */
export const DIALOG_LIMITS = { title: 80, label: 80, value: 500, text: 1000, ok: 32, notify: 500 };

/** A string field of at most `max` characters (absent: `fallback`); anything else is refused. */
function text(v: unknown, what: string, max: number, fallback: string): string {
  if (v === undefined || v === null) return fallback;
  if (typeof v !== "string") throw new PluginRefusal(`${what} must be text`, "invalid");
  if (v.length > max) throw new PluginRefusal(`${what} is longer than ${max} characters`, "invalid");
  return v;
}

/** Read a response body, refusing it past `max` bytes without reading the rest. */
async function readCapped(res: Response, max: number): Promise<Uint8Array> {
  const declared = Number(res.headers.get("content-length"));
  if (Number.isFinite(declared) && declared > max) {
    void res.body?.cancel().catch(() => undefined);
    throw new PluginRefusal(`the answer is larger than ${Math.round(max / 1024 / 1024)} MB (it says ${declared} bytes)`, "too_large");
  }
  if (!res.body) return new Uint8Array(await res.arrayBuffer()).slice(0, max + 1);
  const reader = res.body.getReader();
  const parts: Uint8Array[] = [];
  let n = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    n += value.byteLength;
    if (n > max) {
      void reader.cancel().catch(() => undefined);
      throw new PluginRefusal(`the answer is larger than ${Math.round(max / 1024 / 1024)} MB`, "too_large");
    }
    parts.push(value);
  }
  const out = new Uint8Array(n);
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.byteLength;
  }
  return out;
}

/** What the app gives the plugin system. */
export interface AppBridge {
  scene(): SceneData | null;
  kernel(): KernelClient | null;
  /** Resolves when every edit already sent to the kernel is applied. */
  settled(): Promise<unknown>;
  units(): UnitSystem;
  docName(): string | null;
  selection(): Entity[];
  select(es: Entity[]): void;
  /** Apply a named batch as one undo step; resolves to the outcomes, throws the kernel's reason when refused.
   * `signal` aborts when the plugin stops: a batch still waiting in the edit queue is then dropped. */
  applyBatch(name: string, ops: unknown[], plugin: string, signal?: AbortSignal): Promise<OpOutcome[]>;
  /** A line in the status bar, with optional actions. */
  status(text: string, kind: "info" | "warning" | "error", actions?: { label: string; run: () => void }[]): void;
  /** `signal` aborts when the plugin stops: its dialog closes (or never opens) and answers null. */
  dialog(req: DialogRequest, signal?: AbortSignal): Promise<string | boolean | null>;
  /** Save through the app's dialog. Lane 17: `confirmDownload` is asked before a browser
   * without a save dialog downloads the file; `signal` aborted: nothing is written. */
  saveFile(name: string, bytes: Uint8Array, type: FileType, opts?: { confirmDownload?: () => Promise<boolean>; signal?: AbortSignal }): Promise<{ name: string } | null>;
  /** Lane 17: in the browser, refused (NeedsUserActivation) without the person's click. */
  pickFile(types: FileType[], opts?: { signal?: AbortSignal }): Promise<PickedFile | null>;
  /** Make a tool active ("select", "plugin:<id>/<tool>"). */
  activateTool(id: string): void;
  activeTool(): string;
  fetch(url: string, init: RequestInit): Promise<Response>;
}

export type PluginStatus = "disabled" | "invalid" | HostStatus;

export interface PluginEntry {
  id: string;
  name: string;
  version: string;
  source: PluginSource;
  pkg: PluginPackage | null;
  /** SHA-256 of the package's files (store.ts packageDigest): what a grant is bound to. */
  digest: string;
  /** The bundled sample this plugin takes the place of (same id), by name. */
  replaces?: string;
  /** Why the package cannot be used. */
  errors: string[];
  location?: string;
  enabled: boolean;
  status: PluginStatus;
  lastError: string | null;
  logs: LogLine[];
}

export interface PanelState {
  key: string;
  plugin: string;
  pluginName: string;
  id: string;
  title: string;
  fields: PanelField[];
}

export interface CommandState {
  key: string;
  plugin: string;
  pluginName: string;
  id: string;
  title: string;
  group?: string;
  shortcut?: string;
}

export interface ToolState {
  /** The tool id the app uses: plugin:<plugin id>/<tool id>. */
  key: string;
  plugin: string;
  pluginName: string;
  id: string;
  def: ToolContribution;
  shortcut?: string;
  /** What the plugin last asked the tool to show, as sent. */
  view: unknown;
  /** The same, checked (lane 16: once, when it arrives for the active tool, or when the tool is activated). */
  checked?: PluginToolView | null;
}

export const toolKey = (plugin: string, tool: string) => `plugin:${plugin}/${tool}`;

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

/** What the permission dialog lists for a manifest. */
export function permissionLines(m: Manifest): { permission: Permission; title: string; detail: string }[] {
  return m.permissions.map((p) => ({ permission: p, ...PERMISSION_TEXT[p], detail: p === "net" ? `${PERMISSION_TEXT.net.detail}: ${m.net.join(", ")}` : PERMISSION_TEXT[p].detail }));
}

export class PluginManager {
  private entries = new Map<string, PluginEntry>();
  /** Bytes each plugin downloaded in the current minute (net.fetch). */
  private netUsage = new Map<string, { start: number; bytes: number }>();
  private hosts = new Map<string, PluginHost>();
  private grants: Record<string, Grant> = loadGrants();
  readonly doc = new DocStore();
  readonly commands = new Map<string, CommandState>();
  readonly panels = new Map<string, PanelState>();
  readonly tools = new Map<string, ToolState>();
  private listeners = new Set<() => void>();
  private toolListeners = new Set<(key: string) => void>();
  private ver = 0;
  private bridge: AppBridge;
  private platform: PluginPlatform | null;
  private hostOptions: Partial<HostOptions>;
  private channel: (name: string) => Channel;

  constructor(bridge: AppBridge, platform: PluginPlatform | null, opts: { hostOptions?: Partial<HostOptions>; channel?: (name: string) => Channel } = {}) {
    this.bridge = bridge;
    this.platform = platform;
    this.hostOptions = opts.hostOptions ?? {};
    this.channel = opts.channel ?? workerChannel;
  }

  // ---- React glue -------------------------------------------------------------------------

  subscribe = (fn: () => void): (() => void) => {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  };
  version = (): number => this.ver;
  private notifyTimer: ReturnType<typeof setTimeout> | null = null;
  /** Something changed: the version moves now, the window hears it once per frame (lane 16:
   * a plugin registering in a loop used to re-render the app for every message). */
  private changed(): void {
    this.ver++;
    if (this.notifyTimer !== null) return;
    this.notifyTimer = setTimeout(() => {
      this.notifyTimer = null;
      for (const fn of this.listeners) fn();
    }, 16);
  }
  private viewTimer: ReturnType<typeof setTimeout> | null = null;
  private viewsChanged = new Set<string>();
  /** The checked view of a plugin tool (checked now if it has not been). */
  toolView(key: string): PluginToolView {
    const t = this.tools.get(key);
    if (!t) return checkToolView(null);
    if (!t.checked) t.checked = checkToolView(t.view);
    return t.checked;
  }
  /** Tool views changed (the active plugin tool redraws). */
  onToolView(fn: (key: string) => void): () => void {
    this.toolListeners.add(fn);
    return () => this.toolListeners.delete(fn);
  }

  list(): PluginEntry[] {
    return [...this.entries.values()].sort((a, b) => a.name.localeCompare(b.name));
  }
  get(id: string): PluginEntry | undefined {
    return this.entries.get(id);
  }
  host(id: string): PluginHost | undefined {
    return this.hosts.get(id);
  }
  canInstallFolders(): boolean {
    return !!this.platform?.pickFolder;
  }
  isDesktop(): boolean {
    return this.platform?.kind === "desktop";
  }

  // ---- loading ----------------------------------------------------------------------------

  /** List bundled and installed plugins and start the enabled ones. */
  private inited = false;

  async init(): Promise<void> {
    if (this.inited) {
      // Again after dispose (React's development double mount): start what is enabled.
      await Promise.all(this.list().filter((e) => e.enabled).map((e) => this.start(e.id)));
      return;
    }
    this.inited = true;
    for (const files of Object.values(bundledPlugins())) this.add(files, "bundled");
    if (this.platform) {
      try {
        for (const f of await this.platform.list()) this.addPrepared(await preparePackage(f.files, "installed", f.location), "installed", f.location);
      } catch (e) {
        this.bridge.status(`Plugins: could not read the installed plugins: ${e instanceof Error ? e.message : String(e)}`, "warning");
      }
    }
    this.changed();
    await Promise.all(this.list().filter((e) => e.enabled).map((e) => this.start(e.id)));
  }

  /** Add (or replace) a plugin from its files, checked here (the bundled samples: small).
   * Returns the entry. */
  add(files: Record<string, string>, source: PluginSource, location?: string): PluginEntry {
    const result = loadPackage(files, source, location);
    return this.addPrepared({ result, digest: result.ok ? packageDigest(result.pkg.files) : "" }, source, location);
  }

  /** Add (or replace) a plugin already checked (lane 16: large packages are checked in a
   * worker, checker.ts). */
  private addPrepared(p: Prepared, source: PluginSource, location?: string): PluginEntry {
    const r = p.result;
    if (r.ok && location) r.pkg.location = location;
    const id = r.ok ? r.pkg.manifest.id : (r.id ?? `invalid:${location ?? r.name}`);
    const prev = this.entries.get(id);
    // A bundled sample never replaces what the person installed.
    if (prev && source === "bundled" && prev.source !== "bundled") return prev;
    const grant = this.grants[id];
    const digest = r.ok ? p.digest : "";
    // A plugin taking a bundled sample's place says so (the enable dialog and the pane).
    const replaces = source !== "bundled" ? (prev?.source === "bundled" ? prev.name : prev?.replaces) : undefined;
    const covers = r.ok && grantCovers(grant, r.pkg.manifest.permissions);
    const matches = r.ok && grantMatches(grant, { digest, source, location });
    const entry: PluginEntry = r.ok
      ? { id, name: r.pkg.manifest.name, version: r.pkg.manifest.version, source, pkg: r.pkg, digest, errors: [], location, enabled: covers && matches, status: "disabled", lastError: null, logs: prev?.logs ?? [] }
      : { id, name: r.name, version: "", source, pkg: null, digest, errors: r.errors, location, enabled: false, status: "invalid", lastError: r.errors[0] ?? null, logs: prev?.logs ?? [] };
    if (replaces) entry.replaces = replaces;
    // Asking for more than was granted turns it off until granted again.
    if (r.ok && grant && !covers) entry.lastError = `This version asks for more (${r.pkg.manifest.permissions.filter((p) => !grant.permissions.includes(p)).join(", ")}): enable it again to allow that.`;
    // A grant is for the package the person enabled, not for its id: other files with the
    // same id (a replacement zip, an update, a zip claiming a bundled sample's id) wait for
    // the person to enable them again and see what they ask for.
    else if (r.ok && grant && !matches) entry.lastError = `These are not the files you enabled${replaces ? ` (it takes the place of the bundled ${replaces})` : ""}: enable it again to let it run.`;
    this.entries.set(id, entry);
    return entry;
  }

  /** Install (or load for this session) a plugin from a .zip. */
  async installZip(bytes: Uint8Array, opts: { session: boolean }): Promise<PluginEntry> {
    const { files } = readPluginZip(bytes);
    const source = opts.session ? "session" : "installed";
    const p = await preparePackage(files, source);
    const r = p.result;
    if (!r.ok) throw new Error(`${r.name}: ${r.errors.join("; ")}`);
    const id = r.pkg.manifest.id;
    const was = this.entries.get(id);
    if (was && this.hosts.has(id)) this.stop(id, null);
    let location: string | undefined;
    if (!opts.session) {
      if (!this.platform) throw new Error("plugins cannot be installed here");
      location = await this.platform.install(id, files);
    }
    const entry = this.addPrepared(p, source, location);
    this.changed();
    if (entry.enabled) await this.start(id);
    return entry;
  }

  /** Desktop: load a plugin folder (read again on Reload). */
  async loadFolder(): Promise<PluginEntry | null> {
    if (!this.platform?.pickFolder) throw new Error("folders can be loaded in the desktop app");
    const f = await this.platform.pickFolder();
    if (!f) return null;
    const p = await preparePackage(f.files, "unpacked", f.location);
    const r = p.result;
    if (r.ok && this.hosts.has(r.pkg.manifest.id)) this.stop(r.pkg.manifest.id, null);
    const entry = this.addPrepared(p, "unpacked", f.location);
    this.changed();
    if (entry.enabled) await this.start(entry.id);
    return entry;
  }

  /** Load a plugin from files for this session (tests, developer hooks). */
  async loadSession(files: Record<string, string>): Promise<PluginEntry> {
    const p = await preparePackage(files, "session");
    const r = p.result;
    if (r.ok && this.hosts.has(r.pkg.manifest.id)) this.stop(r.pkg.manifest.id, null);
    const entry = this.addPrepared(p, "session");
    this.changed();
    if (entry.enabled) await this.start(entry.id);
    return entry;
  }

  // ---- lifecycle --------------------------------------------------------------------------

  /** Enable (the person approved the permissions the dialog showed) and start. */
  async enable(id: string): Promise<void> {
    const e = this.entries.get(id);
    if (!e?.pkg) throw new Error(e ? e.errors.join("; ") : `no plugin ${id}`);
    this.grants[id] = { version: e.pkg.manifest.version, permissions: [...e.pkg.manifest.permissions], digest: e.digest, source: e.source, ...(e.location ? { location: e.location } : {}) };
    saveGrants(this.grants);
    e.enabled = true;
    e.lastError = null;
    this.changed();
    await this.start(id);
  }

  disable(id: string): void {
    const e = this.entries.get(id);
    if (!e) return;
    delete this.grants[id];
    saveGrants(this.grants);
    e.enabled = false;
    this.stop(id, null);
    e.status = e.pkg ? "disabled" : "invalid";
    this.changed();
  }

  /** Stop and start again (an unpacked folder is read again first). */
  async reload(id: string): Promise<void> {
    const e = this.entries.get(id);
    if (!e) return;
    this.stop(id, null);
    if (e.source === "unpacked" && e.location && this.platform?.readFolder) {
      try {
        this.addPrepared(await preparePackage(await this.platform.readFolder(e.location), "unpacked", e.location), "unpacked", e.location);
      } catch (err) {
        e.lastError = `could not read ${e.location}: ${err instanceof Error ? err.message : String(err)}`;
      }
    }
    const next = this.entries.get(id)!;
    this.changed();
    if (next.enabled) await this.start(id);
  }

  async uninstall(id: string): Promise<void> {
    const e = this.entries.get(id);
    if (!e) return;
    if (e.source === "bundled") throw new Error(`${e.name} comes with the app: disable it instead`);
    this.disable(id);
    if (e.source === "installed") await this.platform?.uninstall(id, e.location);
    new AppStore(id).clear();
    this.entries.delete(id);
    // A bundled sample of the same id comes back.
    const bundled = Object.values(bundledPlugins()).find((f) => loadPackage(f, "bundled").ok && (loadPackage(f, "bundled") as { pkg: PluginPackage }).pkg.manifest.id === id);
    if (bundled) this.add(bundled, "bundled");
    this.changed();
  }

  async openFolder(id: string): Promise<void> {
    const e = this.entries.get(id);
    if (!e?.location || !this.platform?.openFolder) throw new Error("this plugin has no folder on disk");
    await this.platform.openFolder(e.location);
  }

  /** Stop a running plugin now (the watchdog's "Stop", the pane). */
  stopNow(id: string): void {
    this.stop(id, "stopped by you", "stopped");
  }

  private async start(id: string): Promise<void> {
    const e = this.entries.get(id);
    if (!e?.pkg || this.hosts.has(id)) return;
    const host = new PluginHost(e.pkg, this.channel(`plugin ${id}`), this.delegate, { ...DEFAULT_HOST_OPTIONS, ...this.hostOptions });
    // The console keeps its earlier lines across restarts.
    host.logs.push(...e.logs.slice(-100));
    this.hosts.set(id, host);
    e.status = "starting";
    e.lastError = null;
    this.changed();
    try {
      await host.start(this.bridge.units());
    } catch (err) {
      const text = err instanceof Error ? err.message : String(err);
      e.lastError = host.lastError ?? text;
      this.bridge.status(`${e.name} did not start: ${e.lastError}`, "error");
    }
    this.sync(host);
  }

  private stop(id: string, reason: string | null, status: HostStatus = "stopped"): void {
    const host = this.hosts.get(id);
    if (host) {
      host.stop(reason, status);
      this.hosts.delete(id);
      this.sync(host);
    }
    this.dropContributions(id);
  }

  private dropContributions(id: string): void {
    for (const m of [this.commands, this.panels, this.tools] as Map<string, { plugin: string }>[]) for (const [k, v] of m) if (v.plugin === id) m.delete(k);
    const active = this.bridge.activeTool();
    if (active.startsWith(`plugin:${id}/`)) this.bridge.activateTool("select");
    this.changed();
  }

  /** Copy a host's state into its entry. */
  private sync(host: PluginHost): void {
    const e = this.entries.get(host.id);
    if (!e) return;
    e.status = host.status;
    if (!e.enabled && !host.alive) e.status = e.pkg ? "disabled" : "invalid";
    e.lastError = host.lastError ?? e.lastError;
    e.logs = host.logs;
    if (!host.alive && this.hosts.get(host.id) === host) {
      this.hosts.delete(host.id);
      this.dropContributions(host.id);
    }
    this.changed();
  }

  // ---- calls into plugins ----------------------------------------------------------------------

  /** Events go to every running plugin that listens (and may). */
  emit(name: string, payload: Data): void {
    for (const h of this.hosts.values()) h.emit(name, payload);
  }

  async runCommand(key: string): Promise<void> {
    const c = this.commands.get(key);
    if (!c) return;
    try {
      await this.hosts.get(c.plugin)?.call("command", [c.id], c.title);
    } catch (e) {
      this.bridge.status(`${c.pluginName}: "${c.title}" failed: ${e instanceof Error ? e.message : String(e)}`, "error");
    }
  }

  /** A panel field changed (by the person) or a button was pressed. */
  async panelEvent(key: string, kind: "change" | "action", fieldId: string, value?: FieldValue): Promise<void> {
    const p = this.panels.get(key);
    if (!p) return;
    if (kind === "change") {
      p.fields = p.fields.map((f) => (f.id === fieldId && (f.type === "text" || f.type === "number" || f.type === "select" || f.type === "checkbox") ? ({ ...f, value } as PanelField) : f));
      this.changed();
    }
    try {
      await this.hosts.get(p.plugin)?.call(kind === "change" ? "panel.change" : "panel.action", [p.id, fieldId, valuesOf(p.fields)], kind === "action" ? fieldId : `${fieldId} changed`);
    } catch (e) {
      this.bridge.status(`${p.pluginName}: ${e instanceof Error ? e.message : String(e)}`, "error");
    }
  }

  /** Whether a plugin tool's events may carry the model (its plugin has `model.read`). */
  toolSeesModel(key: string): boolean {
    const t = this.tools.get(key);
    return !!t && !!this.hosts.get(t.plugin)?.has("model.read");
  }

  /** Run a tool callback (the PluginTool in src/tools/plugin.ts calls this). Lane 17: a
   * pointer event reaches a plugin without `model.read` with the model taken out (snap and
   * pick: toolEvent.ts), whatever the caller passed. */
  toolCall(key: string, fn: "tool.activate" | "tool.deactivate" | "tool.event" | "tool.value" | "tool.cancel", arg?: Data): Promise<Data> {
    const t = this.tools.get(key);
    const h = t ? this.hosts.get(t.plugin) : undefined;
    if (!t || !h) return Promise.resolve(null);
    if (fn === "tool.event" && !h.has("model.read")) arg = redactToolEvent(arg, this.toolView(key).inference);
    return h.call(fn, arg === undefined ? [t.id] : [t.id, arg], `${t.def.title}: ${fn.slice(5)}`);
  }

  // ---- requests from plugins --------------------------------------------------------------------

  private delegate = {
    changed: (host: PluginHost) => this.sync(host),
    report: (host: PluginHost, text: string, kind: "warning" | "error", action?: "stop" | "reload") =>
      this.bridge.status(text, kind, action === "stop" ? [{ label: "Stop", run: () => this.stopNow(host.id) }] : action === "reload" ? [{ label: "Reload", run: () => void this.reload(host.id) }] : undefined),
    handle: (host: PluginHost, method: string, params: Data, signal: AbortSignal) => this.handle(host, method, params, signal),
  };

  private async handle(host: PluginHost, method: string, params: Data, signal: AbortSignal = host.signal): Promise<Data> {
    const p = (isObj(params) ? params : {}) as Record<string, unknown>;
    const m = host.pkg.manifest;
    const b = this.bridge;
    const state = () => b.scene()?.state ?? null;
    const declared = (kind: "commands" | "panels" | "tools", id: unknown) => {
      const found = typeof id === "string" ? (m.contributes[kind] as { id: string }[]).find((c) => c.id === id) : undefined;
      if (!found) throw new PluginRefusal(`plugin.json declares no ${kind.slice(0, -1)} ${JSON.stringify(id)} under contributes.${kind}`, "invalid");
      return found;
    };
    const shortcutFor = (kind: "command" | "tool", id: string) => m.contributes.shortcuts.find((s) => s[kind] === id)?.key;
    switch (method) {
      // -- the model (reads wait for edits already on their way)
      case "model.document": {
        await b.settled();
        const st = state();
        return { name: b.docName(), units: b.units(), canUndo: !!st?.can_undo, canRedo: !!st?.can_redo, hash: st?.stats.hash ?? "", context: st?.graph?.context ?? null };
      }
      case "model.stats":
        await b.settled();
        return statsOf(state()) as unknown as Data;
      case "model.solids":
        await b.settled();
        return solidsOf(state()) as unknown as Data;
      case "model.solid":
        await b.settled();
        return solidOf(state(), p.id) as unknown as Data;
      case "model.faces": {
        await b.settled();
        const k = b.kernel();
        if (typeof p.solid !== "number" || !state()?.objects.includes(p.solid)) throw new PluginRefusal(`there is no solid ${JSON.stringify(p.solid)}`, "invalid");
        if (!k) throw new PluginRefusal("the kernel is not loaded");
        return (await k.faces(p.solid)) as unknown as Data;
      }
      case "model.nodes":
        await b.settled();
        return nodesOf(state()) as unknown as Data;
      case "model.definitions":
        await b.settled();
        return definitionsOf(state()) as unknown as Data;
      case "model.tree":
        await b.settled();
        return treeOf(state()) as unknown as Data;
      case "model.selection":
        await b.settled();
        return selectionOf(b.selection(), state()) as unknown as Data;
      case "model.select": {
        await b.settled();
        const st = state();
        let es: Entity[];
        try {
          es = entitiesOfTargets(p.items, st);
        } catch (e) {
          throw new PluginRefusal(e instanceof Error ? e.message : String(e), "invalid");
        }
        const seen = new Set<string>();
        b.select(
          es.flatMap((e) => {
            const s = scopeEntity(e, st);
            const k = s ? JSON.stringify(s) : "";
            if (!s || seen.has(k)) return [];
            seen.add(k);
            return [s];
          }),
        );
        return null;
      }
      case "model.bounds": {
        await b.settled();
        try {
          return boundsOf(p.items === null || p.items === undefined ? null : entitiesOfTargets(p.items, state()), state()) as unknown as Data;
        } catch (e) {
          throw new PluginRefusal(e instanceof Error ? e.message : String(e), "invalid");
        }
      }
      case "model.materials":
        await b.settled();
        return (state()?.materials ?? []) as unknown as Data;
      case "model.cutList": {
        await b.settled();
        const k = b.kernel();
        if (!k) throw new PluginRefusal("the kernel is not loaded");
        const rounding = (p.rounding ?? "sixteenth") as Rounding;
        if (!["sixteenth", "thirty_second", "half_mm"].includes(rounding)) throw new PluginRefusal('rounding must be "sixteenth", "thirty_second" or "half_mm"', "invalid");
        const c: CutList = await k.cutList({ rounding, visible_only: !!p.visibleOnly });
        return cutListOf(c) as unknown as Data;
      }
      case "model.batch": {
        const name = p.name;
        if (typeof name !== "string" || !name.trim() || name.length > 80) throw new PluginRefusal("a batch needs a name of 1 to 80 characters", "invalid");
        if (!Array.isArray(p.ops) || p.ops.length === 0) throw new PluginRefusal("a batch needs at least one op", "invalid");
        if (p.ops.length > MAX_BATCH_OPS) throw new PluginRefusal(`a batch holds at most ${MAX_BATCH_OPS} ops`, "invalid");
        p.ops.forEach((op, i) => {
          if (!isObj(op) || typeof op.op !== "string") throw new PluginRefusal(`op ${i} is not an op (an object with "op")`, "invalid");
          if (FORBIDDEN_OPS.has(op.op)) throw new PluginRefusal(`op ${i}: "${op.op}" cannot be part of a plugin batch (history and navigation belong to the person)`, "invalid");
        });
        try {
          return (await b.applyBatch(name.trim(), p.ops, host.name, host.signal)) as unknown as Data;
        } catch (e) {
          throw new PluginRefusal(e instanceof Error ? e.message : String(e), host.signal.aborted ? "stopped" : "refused");
        }
      }

      // -- the window
      case "ui.registerCommand": {
        const c = declared("commands", p.id) as { id: string; title: string };
        const key = `${m.id}/${c.id}`;
        // Registering the same command again changes nothing (and redraws nothing).
        if (this.commands.has(key)) return null;
        this.commands.set(key, { key, plugin: m.id, pluginName: m.name, id: c.id, title: c.title, group: m.contributes.menus.find((x) => x.command === c.id)?.group, shortcut: shortcutFor("command", c.id) });
        this.changed();
        return null;
      }
      case "ui.registerPanel": {
        const c = declared("panels", p.id) as { id: string; title: string };
        let fields: PanelField[];
        try {
          fields = checkBody(p.body);
        } catch (e) {
          throw new PluginRefusal(e instanceof Error ? e.message : String(e), "invalid");
        }
        const key = `${m.id}/${c.id}`;
        const title = typeof p.title === "string" && p.title ? p.title.slice(0, 64) : c.title;
        const was = this.panels.get(key);
        if (was && was.title === title && JSON.stringify(was.fields) === JSON.stringify(fields)) return null;
        this.panels.set(key, { key, plugin: m.id, pluginName: m.name, id: c.id, title, fields });
        this.changed();
        return null;
      }
      case "ui.updatePanel": {
        const key = `${m.id}/${String(p.id)}`;
        const panel = this.panels.get(key);
        if (!panel) throw new PluginRefusal(`panel ${JSON.stringify(p.id)} is not registered`, "invalid");
        try {
          const fields = p.body !== undefined ? checkBody(p.body) : panel.fields;
          panel.fields = patchFields(fields, { fields: p.fields, values: p.values });
        } catch (e) {
          throw new PluginRefusal(e instanceof Error ? e.message : String(e), "invalid");
        }
        if (typeof p.title === "string" && p.title) panel.title = p.title.slice(0, 64);
        this.changed();
        return null;
      }
      case "ui.registerTool": {
        const def = declared("tools", p.id) as ToolContribution;
        const key = toolKey(m.id, def.id);
        if (this.tools.has(key)) return null;
        this.tools.set(key, { key, plugin: m.id, pluginName: m.name, id: def.id, def, shortcut: shortcutFor("tool", def.id), view: null, checked: null });
        this.changed();
        return null;
      }
      case "ui.toolView": {
        const key = toolKey(m.id, String(p.id));
        const t = this.tools.get(key);
        if (!t) throw new PluginRefusal(`tool ${JSON.stringify(p.id)} is not registered`, "invalid");
        t.view = p.view ?? null;
        t.checked = null;
        // Lane 16: only the active tool's view is checked and drawn now, at most once a frame;
        // another tool's is checked when it is activated.
        if (b.activeTool() !== key) return null;
        t.checked = checkToolView(t.view);
        this.viewsChanged.add(key);
        if (this.viewTimer === null)
          this.viewTimer = setTimeout(() => {
            this.viewTimer = null;
            const keys = [...this.viewsChanged];
            this.viewsChanged.clear();
            for (const k of keys) for (const fn of this.toolListeners) fn(k);
          }, 16);
        return null;
      }
      case "ui.activateTool": {
        const key = toolKey(m.id, String(p.id));
        if (!this.tools.has(key)) throw new PluginRefusal(`tool ${JSON.stringify(p.id)} is not registered`, "invalid");
        b.activateTool(key);
        return null;
      }
      case "ui.notify": {
        // Lane 16: text is required to be text and refused over the limit, never coerced or cut.
        const msg = text(p.text, "notify text", DIALOG_LIMITS.notify, "");
        const kind = p.kind === "warning" || p.kind === "error" ? p.kind : "info";
        if (msg) b.status(`${m.name}: ${msg}`, kind);
        host.log(kind === "error" ? "error" : kind === "warning" ? "warn" : "info", `notify: ${msg}`);
        return null;
      }
      case "ui.prompt": {
        const req: DialogRequest = { kind: "prompt", plugin: m.name, title: text(p.title, "prompt title", DIALOG_LIMITS.title, m.name), label: text(p.label, "prompt label", DIALOG_LIMITS.label, ""), value: text(p.value, "prompt value", DIALOG_LIMITS.value, ""), ok: text(p.ok, "prompt ok", DIALOG_LIMITS.ok, "OK") };
        const r = await b.dialog(req, signal);
        return typeof r === "string" ? r : null;
      }
      case "ui.confirm": {
        const req: DialogRequest = { kind: "confirm", plugin: m.name, title: text(p.title, "confirm title", DIALOG_LIMITS.title, m.name), text: text(p.text, "confirm text", DIALOG_LIMITS.text, ""), ok: text(p.ok, "confirm ok", DIALOG_LIMITS.ok, "OK") };
        const r = await b.dialog(req, signal);
        return r === true;
      }

      // -- storage
      case "storage.get":
      case "storage.set":
      case "storage.delete":
      case "storage.keys": {
        const scope = p.scope === "document" ? "document" : p.scope === "app" ? "app" : null;
        if (!scope) throw new PluginRefusal('storage scope must be "app" or "document"', "invalid");
        try {
          if (scope === "app") {
            const s = new AppStore(m.id);
            if (method === "storage.get") return s.get(p.key) as Data;
            if (method === "storage.set") return s.set(p.key, p.value), null;
            if (method === "storage.delete") return s.delete(p.key), null;
            return s.keys();
          }
          if (method === "storage.get") return this.doc.get(m.id, p.key) as Data;
          if (method === "storage.set") return this.doc.set(m.id, p.key, p.value), null;
          if (method === "storage.delete") return this.doc.delete(m.id, p.key), null;
          return this.doc.keys(m.id);
        } catch (e) {
          throw new PluginRefusal(e instanceof Error ? e.message : String(e), "invalid");
        }
      }

      // -- files, through the app's pickers only
      case "file.save": {
        const name = typeof p.name === "string" ? p.name.trim() : "";
        if (!name || name.length > 200 || /[\\/:*?"<>|\x00-\x1f]/.test(name)) throw new PluginRefusal("file.save needs a plain file name (no folders), like \"report.md\"", "invalid");
        // Lane 17: Windows drops trailing dots and spaces ("setup.exe." is setup.exe), and a
        // name that would run when opened is refused, whatever comes before its last dot.
        if (/[. ]$/.test(name)) throw new PluginRefusal(`file.save: "${name}" ends with a dot or a space`, "invalid");
        const last = /(\.[^.]+)$/.exec(name)?.[1]?.toLowerCase();
        if (last && BLOCKED_SAVE_EXTENSIONS.has(last)) throw new PluginRefusal(`file.save: plugins may not save ${last} files (programs, scripts and pages that run when opened)`, "invalid");
        const bytes = typeof p.data === "string" ? new TextEncoder().encode(p.data) : p.data instanceof Uint8Array ? p.data : null;
        if (!bytes) throw new PluginRefusal("file.save needs data: text or a Uint8Array", "invalid");
        const ext = /(\.[A-Za-z0-9]{1,10})$/.exec(name)?.[1]?.toLowerCase() ?? ".txt";
        const mime = typeof p.mime === "string" && /^[a-z]+\/[a-z0-9.+-]+$/i.test(p.mime) ? p.mime : "application/octet-stream";
        // Lane 17: a browser without a save dialog would put the file in the downloads at
        // once; the app asks first, naming the plugin, the file and its size.
        const confirmDownload = async () => (await b.dialog({ kind: "download", plugin: m.name, name, bytes: bytes.byteLength }, signal)) === true;
        const r = await b.saveFile(name, bytes, { description: typeof p.description === "string" ? p.description.slice(0, 60) : `${ext.slice(1).toUpperCase()} file`, mime, extensions: [ext] }, { confirmDownload, signal });
        host.log("info", r ? `saved ${r.name}` : "save cancelled");
        return r ? { name: r.name } : null;
      }
      case "file.open": {
        const exts = Array.isArray(p.extensions) ? p.extensions.filter((x): x is string => typeof x === "string" && /^\.[A-Za-z0-9]{1,10}$/.test(x)).slice(0, 20) : [];
        let f: PickedFile | null;
        try {
          f = await b.pickFile([{ description: typeof p.description === "string" ? p.description.slice(0, 60) : "Files", mime: "application/octet-stream", extensions: exts.length ? exts : [".txt"] }], { signal });
        } catch (e) {
          // Lane 17: without the person's click the browser never opens the picker (and a
          // file input's promise never settled, holding the plugin's file.open slot).
          if (e instanceof NeedsUserActivation) throw new PluginRefusal(`file.open: ${e.message}`, "user_activation");
          throw e;
        }
        return f && !signal.aborted ? { name: f.name, bytes: f.bytes } : null;
      }

      // -- the network, only the manifest's hosts
      case "net.fetch": {
        let url: URL;
        try {
          url = new URL(String(p.url));
        } catch {
          throw new PluginRefusal(`not an address: ${String(p.url)}`, "invalid");
        }
        const local = url.hostname === "localhost" || url.hostname === "127.0.0.1";
        if (url.protocol !== "https:" && !(local && url.protocol === "http:")) throw new PluginRefusal("plugins reach the network over https only", "permission");
        if (!hostAllowed(m.net, url.hostname)) throw new PluginRefusal(`${m.name} may not reach ${url.hostname}: plugin.json lists ${m.net.join(", ")}`, "permission");
        const method = typeof p.method === "string" ? p.method.toUpperCase() : "GET";
        if (!["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"].includes(method)) throw new PluginRefusal(`method ${method} is not allowed`, "invalid");
        const headers: Record<string, string> = {};
        if (isObj(p.headers)) for (const [k, v] of Object.entries(p.headers)) if (typeof v === "string" && !/^(cookie|host|origin|referer)$/i.test(k)) headers[k] = v;
        const body = typeof p.body === "string" || p.body instanceof Uint8Array ? (p.body as BodyInit) : undefined;
        // Lane 16: bytes a minute per plugin, checked before asking and counted as they arrive.
        const now = Date.now();
        const usage = this.netUsage.get(m.id);
        const used = usage && now - usage.start < 60_000 ? usage : { start: now, bytes: 0 };
        this.netUsage.set(m.id, used);
        if (used.bytes >= NET_LIMITS.perMinute) throw new PluginRefusal(`${m.name} has downloaded ${Math.round(NET_LIMITS.perMinute / 1024 / 1024)} MB in the last minute; wait a little`, "busy");
        const ctl = new AbortController();
        const timer = setTimeout(() => ctl.abort(), NET_LIMITS.seconds * 1000);
        // Stopping the plugin cancels its downloads.
        const stop = () => ctl.abort();
        host.signal.addEventListener("abort", stop);
        try {
          const res = await b.fetch(url.href, { method, headers, body: method === "GET" || method === "HEAD" ? undefined : body, credentials: "omit", redirect: "error", signal: ctl.signal, referrerPolicy: "no-referrer" });
          const buf = await readCapped(res, Math.min(NET_LIMITS.answer, NET_LIMITS.perMinute - used.bytes));
          used.bytes += buf.byteLength;
          const h: Record<string, string> = {};
          res.headers.forEach((v, k) => (h[k] = v));
          host.log("info", `${method} ${url.href} -> ${res.status}`);
          return { status: res.status, statusText: res.statusText, headers: h, body: buf };
        } catch (e) {
          if (e instanceof PluginRefusal) throw e;
          throw new PluginRefusal(`${method} ${url.href} failed: ${e instanceof Error ? e.message : String(e)}`, "refused");
        } finally {
          clearTimeout(timer);
          host.signal.removeEventListener("abort", stop);
        }
      }
    }
    throw new PluginRefusal(`${method} is not handled`, "unknown");
  }

  /** Stop every plugin (the app is closing, or a test resets). */
  dispose(): void {
    for (const id of [...this.hosts.keys()]) this.stop(id, null);
  }
}
