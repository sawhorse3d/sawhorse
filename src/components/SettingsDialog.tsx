// Settings (lane 15, Edit > Settings…): the render service's address. Renders are off until
// one is set and answers its health check; one line says which it is.
import { useState } from "react";
import { checkServiceUrl, gateLine, type GateStatus } from "../credits/gate";

export interface SettingsDialogProps {
  url: string | null;
  status: GateStatus;
  onUrl: (url: string | null) => void;
  onRecheck: () => void;
  onClose: () => void;
}

export function SettingsDialog({ url, status, onUrl, onRecheck, onClose }: SettingsDialogProps) {
  const [text, setText] = useState(url ?? "");
  const [err, setErr] = useState<string | null>(null);
  const save = () => {
    const r = checkServiceUrl(text);
    if (!r.ok) {
      setErr(r.error);
      return;
    }
    setErr(null);
    setText(r.url);
    onUrl(r.url || null);
  };
  return (
    <section
      className="name-dialog settings-dialog"
      role="dialog"
      aria-label="Settings"
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape") onClose();
      }}
    >
      <header>
        <b>Settings</b>
      </header>
      <h5>Renders</h5>
      <label>
        <span>Render service</span>
        <input
          aria-label="Render service address"
          value={text}
          size={34}
          placeholder="https://render.example.com"
          onChange={(e) => setText(e.currentTarget.value)}
          onKeyDown={(e) => e.key === "Enter" && save()}
        />
      </label>
      {err ? <p className="note err">{err}</p> : null}
      <p className="note" data-testid="render-gate" data-status={status}>
        {gateLine(status, url)}
      </p>
      <div className="btnrow">
        <button type="button" className="btn primary" onClick={save}>
          Save
        </button>
        <button type="button" className="btn" disabled={!url} onClick={onRecheck} title="Ask the service again">
          Check again
        </button>
        <button
          type="button"
          className="btn"
          onClick={() => {
            setText("");
            setErr(null);
            onUrl(null);
          }}
        >
          Clear
        </button>
        <button type="button" className="btn" onClick={onClose}>
          Done
        </button>
      </div>
      <p className="note">Saved on this device only. A render sends the current view (depth, edges and a clay image) and your prompt to that service; nothing else leaves the app.</p>
    </section>
  );
}
