// The Plugins pane (lane 11): the running plugins' panels as collapsible sections under each
// plugin's name, then every plugin the app knows (bundled samples, installed, loaded) with
// its state, permissions and last error, and Enable / Disable, Reload, Open folder
// (desktop), Uninstall and the developer console (the plugin's log lines).
import { useState } from "react";
import { PERMISSION_TEXT, type Permission } from "../../packages/plugin-api/src/manifest.ts";
import type { PluginEntry, PluginManager, PluginStatus } from "../plugins/manager";
import type { UnitSystem } from "../lib/units";
import { PluginPanel } from "./PluginPanel";

export interface PluginsPaneProps {
  manager: PluginManager;
  /** Bumped when anything in the manager changes. */
  version: number;
  units: UnitSystem;
  desktop: boolean;
  canLoadFolder: boolean;
  onEnable: (id: string) => void;
  onInstall: (session: boolean) => void;
  onLoadFolder: () => void;
  onStatus: (text: string, kind: "info" | "warning" | "error") => void;
}

const STATUS_WORDS: Record<PluginStatus, string> = {
  disabled: "Off",
  invalid: "Cannot load",
  starting: "Starting",
  running: "Running",
  unresponsive: "Not responding",
  crashed: "Crashed",
  stopped: "Stopped",
  failed: "Did not start",
};

const SOURCE_WORDS: Record<PluginEntry["source"], string> = { bundled: "sample", installed: "installed", unpacked: "folder", session: "this session" };

function Row({ e, props }: { e: PluginEntry; props: PluginsPaneProps }) {
  const { manager } = props;
  const [consoleOpen, setConsoleOpen] = useState(false);
  const m = e.pkg?.manifest;
  const act = (what: string, run: () => Promise<unknown> | void) => {
    try {
      const r = run();
      if (r) void r.catch((err: unknown) => props.onStatus(`${e.name}: could not ${what}: ${err instanceof Error ? err.message : String(err)}`, "error"));
    } catch (err) {
      props.onStatus(`${e.name}: could not ${what}: ${err instanceof Error ? err.message : String(err)}`, "error");
    }
  };
  const alive = e.status === "running" || e.status === "starting" || e.status === "unresponsive";
  return (
    <li className="plugin-row" data-plugin={e.id} data-status={e.status}>
      <div className="plugin-head">
        <b>{e.name}</b>
        {e.version ? <small>{e.version}</small> : null}
        <span className={`plugin-state s-${e.status}`} data-state={e.status}>
          {STATUS_WORDS[e.status]}
        </span>
      </div>
      <div className="plugin-meta">
        {SOURCE_WORDS[e.source]}
        {m ? ` · ${m.author} · ${m.license}` : ""}
      </div>
      {m?.description ? <p className="plugin-desc">{m.description}</p> : null}
      {m ? (
        <ul className="plugin-chips" aria-label="Permissions">
          {m.permissions.length ? (
            m.permissions.map((p) => (
              <li key={p} title={`${PERMISSION_TEXT[p as Permission].title}: ${PERMISSION_TEXT[p as Permission].detail}${p === "net" ? ` (${m.net.join(", ")})` : ""}`}>
                {p}
              </li>
            ))
          ) : (
            <li>no permissions</li>
          )}
        </ul>
      ) : null}
      {e.errors.length ? (
        <ul className="plugin-errors">
          {e.errors.slice(0, 6).map((x, i) => (
            <li key={i}>{x}</li>
          ))}
        </ul>
      ) : e.lastError ? (
        <p className="plugin-error" data-last-error>
          {e.lastError}
        </p>
      ) : null}
      <div className="btnrow">
        {e.pkg ? (
          e.enabled ? (
            <button type="button" className="btn" data-do="disable" onClick={() => act("disable", () => manager.disable(e.id))}>
              Disable
            </button>
          ) : (
            <button type="button" className="btn primary" data-do="enable" onClick={() => props.onEnable(e.id)}>
              Enable…
            </button>
          )
        ) : null}
        {e.enabled ? (
          <button type="button" className="btn" data-do="reload" title={e.source === "unpacked" ? "Read the folder again and restart" : "Restart the plugin"} onClick={() => act("reload", () => manager.reload(e.id))}>
            Reload
          </button>
        ) : null}
        {alive && e.status === "unresponsive" ? (
          <button type="button" className="btn" data-do="stop" onClick={() => act("stop", () => manager.stopNow(e.id))}>
            Stop
          </button>
        ) : null}
        {props.desktop && e.location ? (
          <button type="button" className="btn" data-do="folder" onClick={() => act("open its folder", () => manager.openFolder(e.id))}>
            Open folder
          </button>
        ) : null}
        {e.source !== "bundled" ? (
          <button type="button" className="btn" data-do="uninstall" onClick={() => act("uninstall", () => manager.uninstall(e.id))}>
            {e.source === "installed" ? "Uninstall" : "Remove"}
          </button>
        ) : null}
        <button type="button" className="btn" data-do="console" aria-expanded={consoleOpen} onClick={() => setConsoleOpen((o) => !o)}>
          Developer: console{e.logs.length ? ` (${e.logs.length})` : ""}
        </button>
      </div>
      {consoleOpen ? (
        <pre className="plugin-console" aria-label={`${e.name} console`}>
          {e.logs.length ? e.logs.slice(-200).map((l) => `${new Date(l.at).toLocaleTimeString()} ${l.level.padEnd(5)} ${l.text}`).join("\n") : "(nothing logged yet: console.log in the plugin writes here)"}
        </pre>
      ) : null}
    </li>
  );
}

export function PluginsPane(props: PluginsPaneProps) {
  const { manager, units } = props;
  const entries = manager.list();
  const panels = [...manager.panels.values()];
  return (
    <div className="plugins">
      {panels.map((p) => (
        <details className="plugin-section" key={p.key} open data-panel={p.key}>
          <summary>
            <b>{p.pluginName}</b> <small>{p.title}</small>
          </summary>
          <PluginPanel panelKey={p.key} fields={p.fields} units={units} onChange={(id, v) => void manager.panelEvent(p.key, "change", id, v)} onAction={(id) => void manager.panelEvent(p.key, "action", id)} />
        </details>
      ))}
      <div className="plugin-manage">
        <h4>Installed</h4>
        <ul className="plugin-list">
          {entries.map((e) => (
            <Row key={e.id} e={e} props={props} />
          ))}
        </ul>
        <div className="btnrow">
          <button type="button" className="btn" data-do="install" onClick={() => props.onInstall(false)}>
            Install from .zip…
          </button>
          {props.canLoadFolder ? (
            <button type="button" className="btn" data-do="load-folder" onClick={props.onLoadFolder}>
              Load unpacked folder…
            </button>
          ) : (
            <button type="button" className="btn" data-do="load-session" onClick={() => props.onInstall(true)}>
              Load from .zip for this session…
            </button>
          )}
        </div>
        <p className="note">Plugins run in their own sandboxes and can do only what they asked for when you enabled them. Writing one: docs/PLUGINS.md.</p>
      </div>
    </div>
  );
}
