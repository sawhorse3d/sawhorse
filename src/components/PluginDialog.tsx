// Dialogs for plugins (lane 11), in the naming dialog's style:
// - before a plugin is enabled: what it is and every permission it asks for, Enable / Cancel;
// - a plugin's prompt (a line of text) and confirm (yes or no), titled with its name so a
//   plugin cannot pass itself off as the app.
import { useState } from "react";
import type { DialogRequest } from "../plugins/manager";

export type PluginDialogState =
  | {
      kind: "permissions";
      name: string;
      version: string;
      author: string;
      description: string;
      /** The bundled sample it takes the place of (same id), by name. */
      replaces?: string;
      lines: { permission: string; title: string; detail: string }[];
      resolve: (ok: boolean) => void;
    }
  | { kind: "plugin"; req: DialogRequest; resolve: (v: string | boolean | null) => void };

export function PluginDialog({ state, onClose }: { state: PluginDialogState; onClose: () => void }) {
  const [text, setText] = useState(state.kind === "plugin" && state.req.kind === "prompt" ? state.req.value : "");
  const finish = (v: string | boolean | null) => {
    onClose();
    if (state.kind === "permissions") state.resolve(v === true);
    else state.resolve(v);
  };
  const keys = (e: React.KeyboardEvent) => {
    e.stopPropagation();
    if (e.key === "Escape") finish(state.kind === "permissions" ? false : state.req.kind === "confirm" ? false : null);
  };

  if (state.kind === "permissions") {
    return (
      <section className="name-dialog plugin-dialog" role="dialog" aria-label={`Enable ${state.name}`} onKeyDown={keys}>
        <header>
          <b>Enable {state.name}?</b> <small>
            {state.version} · {state.author}
          </small>
        </header>
        {state.description ? <p className="plugin-desc">{state.description}</p> : null}
        {state.replaces ? (
          <p className="plugin-asks" data-replaces="">
            It has the same id as the bundled sample <b>{state.replaces}</b> and takes its place. It is not the copy that came with Sawhorse.
          </p>
        ) : null}
        {state.lines.length ? (
          <>
            <p className="plugin-asks">It asks to:</p>
            <ul className="plugin-perms" aria-label="Permissions">
              {state.lines.map((l) => (
                <li key={l.permission} data-permission={l.permission}>
                  <b>{l.title}</b> <code>{l.permission}</code>
                  <span>{l.detail}</span>
                </li>
              ))}
            </ul>
          </>
        ) : (
          <p className="plugin-asks">It asks for no permissions: it cannot read or change the model, show anything, keep anything or reach the network.</p>
        )}
        <p className="note">It runs in its own sandbox, apart from the app. Disable it any time in the Plugins pane.</p>
        <div className="btnrow">
          <button type="button" className="btn primary" autoFocus onClick={() => finish(true)}>
            Enable
          </button>
          <button type="button" className="btn" onClick={() => finish(false)}>
            Cancel
          </button>
        </div>
      </section>
    );
  }

  const r = state.req;
  return (
    <section className="name-dialog plugin-dialog" role="dialog" aria-label={`${r.plugin}: ${r.title}`} onKeyDown={keys}>
      <header>
        <b>{r.title}</b> <small>from {r.plugin}</small>
      </header>
      {r.kind === "prompt" ? (
        <label>
          <span>{r.label}</span>
          <input
            autoFocus
            value={text}
            maxLength={500}
            onFocus={(e) => e.currentTarget.select()}
            onChange={(e) => setText(e.currentTarget.value)}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") finish(text);
              if (e.key === "Escape") finish(null);
            }}
          />
        </label>
      ) : (
        <p className="plugin-desc">{r.text}</p>
      )}
      <div className="btnrow">
        <button type="button" className="btn primary" autoFocus={r.kind === "confirm"} onClick={() => finish(r.kind === "prompt" ? text : true)}>
          {r.ok}
        </button>
        <button type="button" className="btn" onClick={() => finish(r.kind === "prompt" ? null : false)}>
          Cancel
        </button>
      </div>
    </section>
  );
}
