// A small naming dialog (Make Component): a title, a text field, OK and Cancel. Enter is
// OK, Esc is Cancel. Shown over the viewport, in the import report's style.
import { useState } from "react";

export interface NameRequest {
  title: string;
  label: string;
  value: string;
  ok: string;
  onOk: (name: string) => void;
}

export function NameDialog({ req, onClose }: { req: NameRequest; onClose: () => void }) {
  const [v, setV] = useState(req.value);
  const ok = () => {
    const name = v.trim();
    if (!name) return;
    onClose();
    req.onOk(name);
  };
  return (
    <section className="name-dialog" role="dialog" aria-label={req.title}>
      <header>
        <b>{req.title}</b>
      </header>
      <label>
        <span>{req.label}</span>
        <input
          autoFocus
          value={v}
          maxLength={64}
          onFocus={(e) => e.currentTarget.select()}
          onChange={(e) => setV(e.currentTarget.value)}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") ok();
            if (e.key === "Escape") onClose();
          }}
        />
      </label>
      <div className="btnrow">
        <button type="button" className="btn primary" disabled={!v.trim()} onClick={ok}>
          {req.ok}
        </button>
        <button type="button" className="btn" onClick={onClose}>
          Cancel
        </button>
      </div>
    </section>
  );
}
