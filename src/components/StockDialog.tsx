// The Stock dialog (lane 9): which stock each material's parts come from, and the library of
// sheets and boards itself (add, edit, delete, back to the built-in set). Every change is a
// kernel op (one undo step each), saved with the model; the nesting follows at once.
import { useState } from "react";
import type { StockDef, StockGrain, StockKind, StockLibrary } from "../kernel/types";
import { parseLength, type UnitSystem } from "../lib/units";
import { fineU, len, lenU, standardActual, stockSize, type Units } from "../nesting/format";

export interface StockDialogProps {
  library: StockLibrary;
  /** The materials (and stock texts) the model's parts use. */
  materials: string[];
  rounding: Units;
  units: UnitSystem;
  onDefine: (stock: StockDef, replace: string | null) => void;
  onDelete: (name: string) => void;
  onDefault: (material: string, stock: string | null) => void;
  onReset: () => void;
  onClose: () => void;
}

interface Draft {
  name: string;
  kind: StockKind;
  width: string;
  length: string;
  thickness: string;
  nominal: string;
  grain: StockGrain;
  /** Lane 15: the actual thickness was typed (not filled in from the nominal). */
  actualSet?: boolean;
  /** Lane 15: the exact sizes of the stock being edited, by the text first shown, so an
   * untouched 18 mm (shown as 45/64 in) saves as 18 mm, not as the shown rounding. */
  exact?: Partial<Record<"width" | "length" | "thickness", [string, number]>>;
}

const IN = 0.0254;
const EMPTY: Draft = { name: "", kind: "sheet", width: "48 in", length: "96 in", thickness: "3/4 in", nominal: "", grain: "length" };

/** The default stock of a material (names compare without case). */
function defaultOf(lib: StockLibrary, material: string): string | null {
  const key = Object.keys(lib.defaults ?? {}).find((m) => m.trim().toLowerCase() === material.trim().toLowerCase());
  return key ? lib.defaults![key] : null;
}

export function StockDialog(p: StockDialogProps) {
  const lib = p.library;
  const [draft, setDraft] = useState<Draft>(EMPTY);
  const [editing, setEditing] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const units: UnitSystem = p.units;
  const edit = (s: StockDef) => {
    setEditing(s.name);
    setErr(null);
    const std = s.nominal ? standardActual(s.kind, s.nominal) : null;
    const width = lenU(s.width, p.rounding);
    const length = lenU(s.length, p.rounding);
    const thickness = fineU(s.thickness, p.rounding);
    setDraft({
      name: s.name,
      kind: s.kind,
      width,
      length,
      thickness,
      nominal: s.nominal ?? "",
      grain: s.grain,
      actualSet: std === null || Math.abs(std * IN - s.thickness) > 1e-9,
      exact: { width: [width, s.width], length: [length, s.length], thickness: [thickness, s.thickness] },
    });
  };
  const save = () => {
    const read = (label: string, t: string, key?: "width" | "length" | "thickness") => {
      const kept = key ? draft.exact?.[key] : undefined;
      if (kept && kept[0] === t) return kept[1];
      const r = parseLength(t, units);
      if (!r.ok) throw new Error(`${label}: ${r.error}`);
      if (r.metres <= 0) throw new Error(`${label} must be more than zero.`);
      return r.metres;
    };
    try {
      const name = draft.name.trim();
      if (!name) throw new Error("Give the stock a name.");
      const stock: StockDef = {
        name,
        kind: draft.kind,
        width: read("Width", draft.width, "width"),
        length: read("Length", draft.length, "length"),
        thickness: read("Actual thickness", draft.thickness, "thickness"),
        nominal: draft.nominal.trim() ? draft.nominal.trim() : null,
        grain: draft.grain,
      };
      if (stock.thickness > Math.min(stock.width, stock.length)) throw new Error("It cannot be thicker than it is wide or long.");
      p.onDefine(stock, editing);
      setDraft(EMPTY);
      setEditing(null);
      setErr(null);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  };
  // Lane 15: a nominal in the standard table fills in the actual thickness until it is typed.
  const fill = (d: Draft): Draft => {
    if (d.actualSet) return d;
    const std = standardActual(d.kind, d.nominal);
    return std === null ? d : { ...d, thickness: fineU(std * IN, p.rounding) };
  };
  const set = (key: keyof Draft, v: string) => {
    if (key === "thickness") setDraft({ ...draft, thickness: v, actualSet: true });
    else if (key === "nominal") setDraft(fill({ ...draft, nominal: v }));
    else setDraft({ ...draft, [key]: v });
  };
  const field = (key: Exclude<keyof Draft, "actualSet" | "exact">, label: string, extra: { placeholder?: string; size?: number; title?: string } = {}) => (
    <label title={extra.title}>
      <span>{label}</span>
      <input aria-label={label} value={draft[key] as string} size={extra.size ?? 6} placeholder={extra.placeholder} onChange={(e) => set(key, e.currentTarget.value)} onKeyDown={(e) => e.key === "Enter" && save()} />
    </label>
  );
  return (
    <section
      className="name-dialog stock-dialog"
      role="dialog"
      aria-label="Stock"
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape") p.onClose();
      }}
    >
      <header>
        <b>Stock</b>
        <small> what parts are cut from</small>
      </header>
      {p.materials.length ? (
        <>
          <h5>Each material's stock</h5>
          <table className="stock-defaults" aria-label="Stock by material">
            <tbody>
              {p.materials.map((m) => (
                <tr key={m}>
                  <td>{m}</td>
                  <td>
                    <select aria-label={`Stock for ${m}`} value={defaultOf(lib, m) ?? ""} onChange={(e) => p.onDefault(m, e.currentTarget.value || null)}>
                      <option value="">Automatic (by kind and thickness)</option>
                      {lib.stocks.map((s) => (
                        <option key={s.name} value={s.name}>
                          {s.name}
                        </option>
                      ))}
                    </select>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      ) : null}
      <h5>Sheets and boards</h5>
      <div className="stock-list">
        <table aria-label="Stock library">
          <tbody>
            {lib.stocks.map((s) => (
              <tr key={s.name} className={editing === s.name ? "sel" : undefined}>
                <td className="nm">
                  {s.name}
                  <small>
                    {s.kind === "sheet" ? `sheet${s.nominal ? ` ${s.nominal} nominal` : ""}` : `board${s.nominal ? ` ${s.nominal}` : ""}`} · {stockSize(s, p.rounding)} actual
                    {s.grain === "none" ? " · no grain" : ""}
                  </small>
                </td>
                <td className="act">
                  <button type="button" className="linkish" onClick={() => edit(s)} aria-label={`Edit ${s.name}`}>
                    Edit
                  </button>
                  <button type="button" className="linkish" onClick={() => p.onDelete(s.name)} aria-label={`Delete ${s.name}`}>
                    Delete
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <h5>{editing ? `Edit ${editing}` : "Add stock"}</h5>
      <div className="stock-form">
        {field("name", "Name", { placeholder: "3/4 Baltic birch 5 x 5", size: 18 })}
        <label>
          <span>Kind</span>
          <select aria-label="Kind" value={draft.kind} onChange={(e) => setDraft(fill({ ...draft, kind: e.currentTarget.value as StockKind }))}>
            <option value="sheet">Sheet</option>
            <option value="board">Board</option>
          </select>
        </label>
        {field("width", "Width")}
        {field("length", "Length")}
        {field("nominal", "Nominal", { placeholder: draft.kind === "board" ? "4/4" : "3/4", size: 4, title: "What the yard calls it: 4/4, 8/4 for boards (board feet are counted on it), 3/4 for plywood" })}
        {field("thickness", "Actual thickness", { title: "What it measures. Filled in from the nominal (3/4 ply is 23/32 in, 4/4 S4S is 3/4 in); type to override" })}
        <label>
          <span>Grain</span>
          <select aria-label="Grain" value={draft.grain} onChange={(e) => setDraft({ ...draft, grain: e.currentTarget.value as StockGrain })}>
            <option value="length">Along the length</option>
            <option value="none">None (MDF)</option>
          </select>
        </label>
      </div>
      {err ? <p className="note err">{err}</p> : null}
      <div className="btnrow">
        <button type="button" className="btn primary" onClick={save}>
          {editing ? "Save stock" : "Add stock"}
        </button>
        {editing ? (
          <button
            type="button"
            className="btn"
            onClick={() => {
              setEditing(null);
              setDraft(EMPTY);
              setErr(null);
            }}
          >
            Cancel edit
          </button>
        ) : null}
        <button type="button" className="btn" onClick={p.onReset} title="Back to the built-in sheets and boards, kerf 1/8 in, no trim, table saw">
          Built-in stock
        </button>
        <button type="button" className="btn" onClick={p.onClose}>
          Done
        </button>
      </div>
      <p className="note">
        Sizes in {units === "mm" || units === "cm" || units === "m" ? units : "inches"} unless typed with a unit. Lengths run along the grain. Sizes are actual (what the stock measures); board feet are counted on a board's nominal thickness. A bare number like {len(0.75 * IN, p.rounding)} is read in the model's units.
      </p>
    </section>
  );
}
