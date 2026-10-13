// A plugin's panel, drawn by the app from the plugin's declarative fields (src/plugins/panel.ts
// checked them): text, number (lengths in the document's units), select, checkbox, button,
// table, note, image. No plugin markup reaches the page.
import { memo, useEffect, useRef, useState } from "react";
import type { FieldValue, PanelField } from "../plugins/panel";
import { formatLength, parseLength, type UnitSystem } from "../lib/units";

export interface PluginPanelProps {
  /** For ids: pane-unique prefix. */
  panelKey: string;
  fields: PanelField[];
  units: UnitSystem;
  onChange: (id: string, value: FieldValue) => void;
  onAction: (id: string) => void;
}

/**
 * A number field: lengths are typed and shown in the document's units, committed on Enter or
 * blur. Enter that commits also hands the keyboard back to the model (the field loses focus),
 * so the next tool key ("r", "p") picks a tool instead of typing into the field; a value that
 * cannot be read keeps the focus, with its error, to be fixed.
 */
function NumberField({ f, units, onChange, domId }: { f: Extract<PanelField, { type: "number" }>; units: UnitSystem; onChange: (v: number | null) => void; domId: string }) {
  const show = (v: number | null) => (v === null ? "" : f.unit === "length" ? formatLength(v, units).replace(/^~ /, "") : String(v));
  const [text, setText] = useState(show(f.value));
  const [error, setError] = useState<string | null>(null);
  /** Set while Enter's own commit blurs the field, so the blur does not commit again. */
  const committed = useRef(false);
  useEffect(() => {
    setText(show(f.value));
    setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [f.value, units, f.unit]);
  /** Commit the typed text; false when it cannot be read (the error shows). */
  const commit = (): boolean => {
    const fail = (why: string) => (setError(why), false);
    const t = text.trim();
    if (t === "") return (setText(show(f.value)), true);
    let v: number;
    if (f.unit === "length") {
      const r = parseLength(t, units);
      if (!r.ok) return fail(r.error);
      v = r.metres;
    } else {
      v = Number(t);
      if (!Number.isFinite(v)) return fail(`"${t}" is not a number`);
      if (f.unit === "count") v = Math.round(v);
    }
    if (f.min !== undefined && v < f.min) return fail(`At least ${f.unit === "length" ? formatLength(f.min, units) : f.min}.`);
    if (f.max !== undefined && v > f.max) return fail(`At most ${f.unit === "length" ? formatLength(f.max, units) : f.max}.`);
    setError(null);
    if (v !== f.value) onChange(v);
    else setText(show(v));
    return true;
  };
  return (
    <label className="pf-field" htmlFor={domId}>
      <span>{f.label}</span>
      <input
        id={domId}
        type="text"
        inputMode="decimal"
        data-field={f.id}
        value={text}
        disabled={f.disabled}
        aria-invalid={!!error}
        onChange={(e) => setText(e.currentTarget.value)}
        onBlur={() => {
          if (committed.current) committed.current = false;
          else commit();
        }}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter" && commit()) {
            committed.current = true;
            e.currentTarget.blur();
          }
          if (e.key === "Escape") {
            // Back to the value it had, and no error left to vanish under the next click.
            setText(show(f.value));
            setError(null);
          }
        }}
      />
      {error ? <em className="pf-error">{error}</em> : null}
    </label>
  );
}

function TextField({ f, onChange, domId }: { f: Extract<PanelField, { type: "text" }>; onChange: (v: string) => void; domId: string }) {
  const [text, setText] = useState(f.value);
  const committed = useRef(false);
  useEffect(() => setText(f.value), [f.value]);
  const commit = () => text !== f.value && onChange(text);
  return (
    <label className="pf-field" htmlFor={domId}>
      <span>{f.label}</span>
      <input
        id={domId}
        type="text"
        data-field={f.id}
        value={text}
        placeholder={f.placeholder}
        disabled={f.disabled}
        maxLength={2000}
        onChange={(e) => setText(e.currentTarget.value)}
        onBlur={() => {
          if (committed.current) committed.current = false;
          else commit();
        }}
        onKeyDown={(e) => {
          e.stopPropagation();
          // Enter commits and hands the keyboard back to the model, as in a number field.
          if (e.key === "Enter") {
            commit();
            committed.current = true;
            e.currentTarget.blur();
          }
        }}
      />
    </label>
  );
}

function PluginPanelView({ panelKey, fields, units, onChange, onAction }: PluginPanelProps) {
  const domId = (id: string) => `pp-${panelKey.replace(/[^A-Za-z0-9_-]/g, "_")}-${id}`;
  const buttons: Extract<PanelField, { type: "button" }>[] = [];
  const out: React.ReactNode[] = [];
  const flush = (k: string) => {
    if (!buttons.length) return;
    const row = buttons.splice(0);
    out.push(
      <div className="btnrow" key={`b-${k}`}>
        {row.map((b) => (
          <button key={b.id} type="button" className={`btn${b.primary ? " primary" : ""}`} data-action={b.id} disabled={b.disabled} onClick={() => onAction(b.id)}>
            {b.label}
          </button>
        ))}
      </div>,
    );
  };
  fields.forEach((f, i) => {
    const k = f.id ?? `${f.type}-${i}`;
    if (f.type === "button") {
      buttons.push(f);
      return;
    }
    flush(k);
    switch (f.type) {
      case "number":
        out.push(<NumberField key={k} f={f} units={units} domId={domId(f.id)} onChange={(v) => onChange(f.id, v)} />);
        return;
      case "text":
        out.push(<TextField key={k} f={f} domId={domId(f.id)} onChange={(v) => onChange(f.id, v)} />);
        return;
      case "select":
        out.push(
          <label className="pf-field" key={k} htmlFor={domId(f.id)}>
            <span>{f.label}</span>
            <select id={domId(f.id)} data-field={f.id} value={f.value} disabled={f.disabled} onChange={(e) => onChange(f.id, e.currentTarget.value)}>
              {f.options.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
          </label>,
        );
        return;
      case "checkbox":
        out.push(
          <label className="pf-check" key={k}>
            <input type="checkbox" data-field={f.id} checked={f.value} disabled={f.disabled} onChange={(e) => onChange(f.id, e.currentTarget.checked)} />
            <span>{f.label}</span>
          </label>,
        );
        return;
      case "table":
        out.push(
          <div className="pf-table-wrap" key={k}>
            <table className="pf-table" data-table={f.id}>
              <thead>
                <tr>
                  {f.columns.map((c, ci) => (
                    <th key={ci} style={{ textAlign: f.align?.[ci] ?? "left" }}>
                      {c}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {f.rows.map((r, ri) => (
                  <tr key={ri}>
                    {r.map((cell, ci) => (
                      <td key={ci} style={{ textAlign: f.align?.[ci] ?? "left" }} title={cell.length > 18 ? cell : undefined}>
                        {cell}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>,
        );
        return;
      case "note":
        out.push(
          <p className={`pf-note ${f.tone}`} key={k} data-note={f.id}>
            {f.text}
          </p>,
        );
        return;
      case "image":
        out.push(<img className="pf-image" key={k} src={f.src} alt={f.alt} style={f.width ? { width: Math.min(f.width, 276) } : undefined} />);
        return;
    }
  });
  flush("end");
  return <div className="plugin-panel">{out}</div>;
}

/**
 * Lane 16: a panel redraws only when its own fields (or the units) change, not on every
 * change anywhere in the plugin system (another plugin's request used to redraw every
 * table cell of every panel). The callbacks name the panel by its key, so the first ones
 * stay right.
 */
export const PluginPanel = memo(PluginPanelView, (a, b) => a.panelKey === b.panelKey && a.fields === b.fields && a.units === b.units);
