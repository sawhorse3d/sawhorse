// The plugin system in the app (lane 11): one PluginManager for the session, wired to the
// app through an AppBridge, and everything the window shows for it: the rail's plugin
// group, the Plugins menu, the Plugins pane's props, the dialogs (permissions before
// enabling, a plugin's prompt and confirm), plugin shortcuts, and the events plugins listen
// to (selection, document, tool, units).
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import type { Data } from "../../packages/plugin-api/src/rpc.ts";
import type { MenuItem } from "../components/TopBar";
import { PluginDialog, type PluginDialogState } from "../components/PluginDialog";
import type { PluginsPaneProps } from "../components/PluginsPane";
import type { Entity, SceneData } from "../kernel/types";
import { pickFile } from "../io/files";
import { setPluginTools, type PluginToolId, type ToolDef, type ToolId } from "../lib/tools";
import type { UnitSystem } from "../lib/units";
import { PluginTool } from "../tools/plugin";
import type { Tool, ToolHost } from "../tools/tool";
import { PluginManager, permissionLines, type AppBridge, type DialogRequest } from "./manager";
import { selectionOf } from "./model";
import { pluginPlatform } from "./platform";

export interface UsePluginsArgs {
  bridge: Omit<AppBridge, "dialog">;
  toolHost: ToolHost;
  scene: SceneData | null;
  selection: Entity[];
  toolId: ToolId;
  units: UnitSystem;
  /** Bring the Plugins pane forward. */
  showPane: () => void;
}

const ZIP = { description: "Sawhorse plugin (.zip)", mime: "application/zip", extensions: [".zip"] };

export function usePlugins({ bridge, toolHost, scene, selection, toolId, units, showPane }: UsePluginsArgs) {
  const [dialog, setDialog] = useState<PluginDialogState | null>(null);
  const dialogQueue = useRef<PluginDialogState[]>([]);
  const openDialog = useCallback((d: PluginDialogState) => {
    setDialog((cur) => {
      if (cur) {
        dialogQueue.current.push(d);
        return cur;
      }
      return d;
    });
  }, []);
  const closeDialog = useCallback(() => setDialog(dialogQueue.current.shift() ?? null), []);
  /** Take a dialog away, shown or queued (its plugin stopped). */
  const dropDialog = useCallback((d: PluginDialogState) => {
    dialogQueue.current = dialogQueue.current.filter((x) => x !== d);
    setDialog((cur) => (cur === d ? (dialogQueue.current.shift() ?? null) : cur));
  }, []);

  const managerRef = useRef<PluginManager | null>(null);
  if (!managerRef.current) {
    managerRef.current = new PluginManager(
      {
        ...bridge,
        // Lane 16: a plugin that stops takes its dialog with it (shown or still queued).
        dialog: (req: DialogRequest, signal?: AbortSignal) =>
          new Promise((resolve) => {
            if (signal?.aborted) return resolve(null);
            const d: PluginDialogState = { kind: "plugin", req, resolve };
            openDialog(d);
            signal?.addEventListener(
              "abort",
              () => {
                dropDialog(d);
                resolve(null);
              },
              { once: true },
            );
          }),
      },
      pluginPlatform(),
    );
  }
  const manager = managerRef.current;
  const version = useSyncExternalStore(manager.subscribe, manager.version);

  useEffect(() => {
    void manager.init();
    // Developer and test hooks (read the state, load a plugin's files for this session, enable
    // one without the dialog). Lane 17: `enable` grants a whole manifest with no consent, so
    // any script in the page could have used it; the hooks exist only in the dev server, the
    // unit tests and the end-to-end build (vite.config.ts, testHooks), never in a release.
    if (__SAWHORSE_TEST_HOOKS__) {
      const w = window as unknown as { __sawhorse?: Record<string, unknown> };
      w.__sawhorse = {
        ...(w.__sawhorse ?? {}),
        plugins: {
          // scripts/check-test-hooks.mjs looks for this string in a build.
          build: "sawhorse-test-hooks",
          list: () => manager.list().map((e) => ({ id: e.id, name: e.name, version: e.version, source: e.source, enabled: e.enabled, status: e.status, lastError: e.lastError, errors: e.errors, logs: e.logs.map((l) => `${l.level}: ${l.text}`) })),
          loadSession: async (files: Record<string, string>) => (await manager.loadSession(files)).id,
          enable: (id: string) => manager.enable(id),
          disable: (id: string) => manager.disable(id),
          commands: () => [...manager.commands.keys()],
          run: (key: string) => manager.runCommand(key),
        },
      };
    }
    return () => manager.dispose();
  }, [manager]);

  // ---- tools --------------------------------------------------------------------------------
  const toolsRef = useRef(new Map<string, PluginTool>());
  const toolDefs = useMemo<ToolDef[]>(() => {
    void version;
    return [...manager.tools.values()].map((t) => ({ id: t.key as PluginToolId, label: t.def.title, key: "", keyCap: t.shortcut ?? "", hint: t.def.hint ?? `${t.def.title} (${t.pluginName})`, vcb: t.def.vcb ?? null, icon: t.def.icon, plugin: t.pluginName }));
  }, [manager, version]);
  setPluginTools(toolDefs);
  useEffect(() => {
    const live = new Set(manager.tools.keys());
    for (const [k, t] of toolsRef.current)
      if (!live.has(k)) {
        t.dispose();
        toolsRef.current.delete(k);
      }
  }, [manager, version]);
  const toolFor = useCallback(
    (id: ToolId): Tool | null => {
      if (!id.startsWith("plugin:") || !manager.tools.has(id)) return null;
      let t = toolsRef.current.get(id);
      if (!t) {
        t = new PluginTool(toolHost, manager, id as PluginToolId);
        toolsRef.current.set(id, t);
      }
      return t;
    },
    [manager, toolHost],
  );

  // ---- events plugins listen to -------------------------------------------------------------
  const hash = scene?.state.stats.hash ?? "";
  const logLen = scene?.state.log_len ?? 0;
  useEffect(() => {
    if (!scene) return;
    const t = setTimeout(() => manager.emit("documentChanged", { hash, canUndo: !!scene.state.can_undo }), 60);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [manager, hash, logLen]);
  useEffect(() => {
    manager.emit("selectionChanged", { selection: selectionOf(selection, scene?.state ?? null) as unknown as Data });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [manager, selection]);
  useEffect(() => manager.emit("toolChanged", { tool: toolId }), [manager, toolId]);
  useEffect(() => manager.emit("unitsChanged", { units }), [manager, units]);

  /** New or opened: document-scoped stores reset (or come from the file's plugin-data.json,
   * which the kernel read, lane 16), plugins hear it. */
  const documentOpened = useCallback(
    (name: string | null, pluginData?: string | null) => {
      if (typeof pluginData === "string") manager.doc.loadJson(pluginData);
      else manager.doc.clear();
      manager.emit("documentOpened", { name });
    },
    [manager],
  );

  // ---- installing -------------------------------------------------------------------------------
  const installZip = useCallback(
    async (session: boolean) => {
      try {
        const f = await pickFile([ZIP]);
        if (!f) return;
        const e = await manager.installZip(f.bytes, { session });
        bridge.status(`${e.name} ${e.version} ${session ? "loaded for this session" : "installed"}${e.enabled ? "" : ": enable it in the Plugins pane (its permissions are shown first)"}.`, "info");
        showPane();
      } catch (err) {
        bridge.status(`Could not ${session ? "load" : "install"} the plugin: ${err instanceof Error ? err.message : String(err)}`, "error");
      }
    },
    [manager, bridge, showPane],
  );
  const loadFolder = useCallback(async () => {
    try {
      const e = await manager.loadFolder();
      if (e) bridge.status(e.pkg ? `${e.name} loaded from its folder${e.enabled ? "" : ": enable it to run it"}.` : `${e.name}: ${e.errors[0]}`, e.pkg ? "info" : "error");
      showPane();
    } catch (err) {
      bridge.status(`Could not load the folder: ${err instanceof Error ? err.message : String(err)}`, "error");
    }
  }, [manager, bridge, showPane]);

  /** Enable after showing what the plugin asks for. */
  const askEnable = useCallback(
    (id: string) => {
      const e = manager.get(id);
      if (!e?.pkg) return;
      const m = e.pkg.manifest;
      openDialog({
        kind: "permissions",
        name: m.name,
        version: m.version,
        author: m.author,
        description: m.description,
        ...(e.replaces ? { replaces: e.replaces } : {}),
        lines: permissionLines(m),
        resolve: (ok) => {
          if (ok) void manager.enable(id).catch((err) => bridge.status(`Could not enable ${m.name}: ${err instanceof Error ? err.message : String(err)}`, "error"));
        },
      });
    },
    [manager, openDialog, bridge],
  );

  // ---- the Plugins menu and shortcuts -------------------------------------------------------------
  const menu = useMemo<MenuItem[]>(() => {
    void version;
    const byPlugin = new Map<string, MenuItem[]>();
    for (const c of manager.commands.values()) {
      const list = byPlugin.get(c.pluginName) ?? [];
      list.push({ label: c.title, keys: c.shortcut, onSelect: () => void manager.runCommand(c.key) });
      byPlugin.set(c.pluginName, list);
    }
    for (const t of manager.tools.values()) {
      const list = byPlugin.get(t.pluginName) ?? [];
      list.push({ label: `${t.def.title} (tool)`, keys: t.shortcut, onSelect: () => bridge.activateTool(t.key) });
      byPlugin.set(t.pluginName, list);
    }
    const items: MenuItem[] = [...byPlugin.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([label, items]) => ({ label, items }));
    if (!items.length) items.push({ label: "No plugin commands yet", disabled: true, onSelect: () => undefined });
    items.push("-", { label: "Manage Plugins…", onSelect: showPane });
    items.push({ label: "Install from .zip…", title: "Install a plugin (a .zip of its folder); it is kept for next time", onSelect: () => void installZip(false) });
    if (manager.canInstallFolders()) items.push({ label: "Load Unpacked Folder…", title: "A plugin folder you are working on: Reload reads it again", onSelect: () => void loadFolder() });
    else items.push({ label: "Load from .zip for This Session…", title: "Run a plugin until this tab closes, without installing it", onSelect: () => void installZip(true) });
    return items;
  }, [manager, version, bridge, showPane, installZip, loadFolder]);

  /** A plugin shortcut (Alt+Shift+C ...): returns whether it was one. */
  const shortcut = useCallback(
    (e: KeyboardEvent): boolean => {
      if (!(e.ctrlKey || e.altKey || e.metaKey) || e.key.length === 0) return false;
      const key = /^F\d+$/.test(e.key) ? e.key : e.code.startsWith("Key") ? e.code.slice(3) : e.code.startsWith("Digit") ? e.code.slice(5) : e.key.toUpperCase();
      const combo = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Meta", key].filter(Boolean).join("+");
      for (const c of manager.commands.values())
        if (c.shortcut === combo) {
          e.preventDefault();
          void manager.runCommand(c.key);
          return true;
        }
      for (const t of manager.tools.values())
        if (t.shortcut === combo) {
          e.preventDefault();
          bridge.activateTool(t.key);
          return true;
        }
      return false;
    },
    [manager, bridge],
  );

  const pane: PluginsPaneProps = {
    manager,
    version,
    units,
    desktop: manager.isDesktop(),
    canLoadFolder: manager.canInstallFolders(),
    onEnable: askEnable,
    onInstall: (session) => void installZip(session),
    onLoadFolder: () => void loadFolder(),
    onStatus: bridge.status,
  };

  const dialogs: ReactNode = dialog ? <PluginDialog state={dialog} onClose={closeDialog} /> : null;

  return { manager, version, toolDefs, toolFor, menu, shortcut, pane, dialogs, documentOpened };
}
