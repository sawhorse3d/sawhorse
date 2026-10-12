// The Nesting section of the Cut list pane (lane 9): the cut list's parts laid out on real
// stock by the kernel. Saw (table-saw guillotine or the tightest maxrects layout), kerf and
// trim, a Stock… button for the library; three tiles (sheets, boards, yield); anything no
// stock takes, with why; then one diagram per sheet or board, drawn to scale, each part
// labelled with its tag (hover: its name and size, and the part lights up in the model;
// click: select it), the cuts in order underneath; and the SVG, DXF and PDF exports.
import { useEffect, useMemo, useState } from "react";
import type { NestAlgorithm, Nesting, NestSettings, Placement, StockPiece } from "../kernel/types";
import { parseLength, type UnitSystem } from "../lib/units";
import { diagram } from "../nesting/diagram";
import { ALGORITHMS, cutText, dims, lenU, type Units, nestNote, nestSummary, pieceNumber, pieceYield, reasonText } from "../nesting/format";

export interface NestingSectionProps {
  nesting: Nesting | null;
  /** Why there is no nesting (the kernel refused, or is not loaded). */
  problem?: string | null;
  /** Working it out again (the diagrams are of the last result). */
  busy?: boolean;
  rounding: Units;
  /** How typed kerf and trim are read (a bare number is in these units). */
  units: UnitSystem;
  onSettings: (s: NestSettings) => void;
  onStock: () => void;
  /** The pointer is over a part's rectangle (null: over none). */
  onHover: (p: Placement | null) => void;
  onSelect: (p: Placement) => void;
  /** Save SVG drawings (one piece, or every piece); without it there are no file buttons. */
  onSvg?: (piece: StockPiece | null) => void;
  onDxf?: (flavour: "lwpolyline" | "r12") => void;
  onPdf?: () => void;
  /** The tag of the part lit up from the model side (its rectangle shows it). */
  lit?: string | null;
}

/** Kerf or trim typed in a field: committed on Enter, on leaving the field, or after a pause. */
function LengthField({ label, value, units, rounding, max, onCommit }: { label: string; value: number; units: UnitSystem; rounding: Units; max: number; onCommit: (m: number) => void }) {
  const shown = lenU(value, rounding);
  const [text, setText] = useState(shown);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => setText(shown), [shown]);
  const commit = (t: string) => {
    if (t.trim() === shown) return setErr(null);
    const r = parseLength(t, units);
    if (!r.ok) return setErr(r.error);
    if (r.metres < 0 || r.metres > max) return setErr(`${label}: 0 to ${lenU(max, rounding)}.`);
    setErr(null);
    if (Math.abs(r.metres - value) > 1e-9) onCommit(r.metres);
  };
  // A short pause after typing re-nests too.
  useEffect(() => {
    if (text === shown) return;
    const t = window.setTimeout(() => commit(text), 700);
    return () => window.clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [text]);
  return (
    <label className={err ? "err" : undefined} title={err ?? `${label} (type a length: 1/8, 3/32, 3mm)`}>
      <span>{label}</span>
      <input
        aria-label={label}
        value={text}
        size={5}
        onChange={(e) => setText(e.currentTarget.value)}
        onBlur={(e) => commit(e.currentTarget.value)}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") commit(e.currentTarget.value);
          if (e.key === "Escape") {
            setText(shown);
            setErr(null);
          }
        }}
      />
    </label>
  );
}

const KERF_MAX = 0.5 * 0.0254;
const TRIM_MAX = 4 * 0.0254;

function PieceDiagram({ piece, rounding, lit, onHover, onSelect }: { piece: StockPiece; rounding: Units; lit?: string | null; onHover: (p: Placement | null) => void; onSelect: (p: Placement) => void }) {
  const d = useMemo(() => diagram(piece), [piece]);
  const [over, setOver] = useState<string | null>(null);
  const S = 1000; // millimetres: friendlier numbers in the viewBox
  const font = Math.min(d.width, d.height) * S * 0.11;
  return (
    <svg
      className="nest-diagram"
      viewBox={`0 0 ${d.width * S} ${d.height * S}`}
      role="img"
      aria-label={`${pieceNumber(piece)}: ${piece.placements.length} part${piece.placements.length === 1 ? "" : "s"}, ${pieceYield(piece, rounding)}`}
      onMouseLeave={() => {
        setOver(null);
        onHover(null);
      }}
    >
      <rect className="stock" x={0} y={0} width={d.width * S} height={d.height * S} />
      {d.usable ? <rect className="trim" x={d.usable.x * S} y={d.usable.y * S} width={d.usable.w * S} height={d.usable.h * S} /> : null}
      {d.parts.map((q) => {
        const on = over === q.p.tag || lit === q.p.tag;
        const fs = Math.max(0, Math.min(font, (q.w * S * 0.9) / (q.p.tag.length * 0.62), q.h * S * 0.8));
        return (
          <g
            key={q.p.tag}
            className={`part${on ? " on" : ""}`}
            data-tag={q.p.tag}
            onMouseEnter={() => {
              setOver(q.p.tag);
              onHover(q.p);
            }}
            onClick={() => onSelect(q.p)}
          >
            <title>{`${q.p.tag} ${q.p.name}: ${dims(q.p.size[1], q.p.size[2], rounding)}${q.p.turned ? " (turned)" : ""}. Click to select it in the model.`}</title>
            <rect x={q.x * S} y={q.y * S} width={q.w * S} height={q.h * S} />
            {fs > font * 0.18 ? (
              <text x={(q.x + q.w / 2) * S} y={(q.y + q.h / 2) * S + fs * 0.35} fontSize={fs}>
                {q.p.tag}
              </text>
            ) : null}
          </g>
        );
      })}
      {d.cuts.map((c) => (
        <line key={c.c.n} className={`cut${c.c.trim ? " trimcut" : ""}`} x1={c.x1 * S} y1={c.y1 * S} x2={c.x2 * S} y2={c.y2 * S} />
      ))}
    </svg>
  );
}

export function NestingSection(p: NestingSectionProps) {
  const n = p.nesting;
  const r = p.rounding;
  const s = n ? nestSummary(n, r) : null;
  const settings = n?.settings;
  const set = (patch: Partial<NestSettings>) => settings && p.onSettings({ ...settings, ...patch });
  return (
    <section className="nesting" aria-label="Nesting" data-testid="nesting">
      <header className="nest-head">
        <h4>Nesting</h4>
        <button type="button" className="btn" onClick={p.onStock} disabled={!n} title="The sheets and boards parts are cut from, and which stock each material uses">
          Stock…
        </button>
      </header>
      {settings ? (
        <div className="cut-controls nest-controls">
          <label title={ALGORITHMS.find((a) => a.id === settings.algorithm)?.title}>
            <span>Saw</span>
            <select aria-label="Nesting algorithm" value={settings.algorithm} onChange={(e) => set({ algorithm: e.currentTarget.value as NestAlgorithm })}>
              {ALGORITHMS.map((a) => (
                <option key={a.id} value={a.id} title={a.title}>
                  {a.label}
                </option>
              ))}
            </select>
          </label>
          <LengthField label="Kerf" value={settings.kerf} units={p.units} rounding={r} max={KERF_MAX} onCommit={(kerf) => set({ kerf })} />
          <LengthField label="Trim" value={settings.trim} units={p.units} rounding={r} max={TRIM_MAX} onCommit={(trim) => set({ trim })} />
        </div>
      ) : null}
      {p.problem ? <p className="note err">{p.problem}</p> : null}
      {!n ? (
        p.problem ? null : <p className="note">Nesting the parts…</p>
      ) : (
        <div className={p.busy ? "nest-body busy" : "nest-body"} aria-busy={p.busy}>
          <div className="sum nest-sum">
            <div data-testid="tile-nest-sheets">
              <b>{s!.sheets}</b>
              <span>{s!.sheetsLabel}</span>
            </div>
            <div data-testid="tile-nest-boards">
              <b>{s!.boards}</b>
              <span>{s!.boardsLabel}</span>
            </div>
            <div data-testid="tile-nest-yield" className="wide">
              <b>{s!.yieldText}</b>
              <span>{s!.yieldLabel}</span>
            </div>
          </div>
          {n.unplaced.length ? (
            <div className="nest-unplaced" role="alert" data-testid="nest-unplaced">
              <b>
                {n.unplaced.length} part{n.unplaced.length === 1 ? "" : "s"} on no stock
              </b>
              <ul>
                {n.unplaced.map((u) => (
                  <li key={u.tag}>
                    <span className="tag">{u.tag}</span> {u.name}: {reasonText(u)}
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
          {n.pieces.length === 0 && !n.unplaced.length ? <p className="placeholder">No parts to nest yet: the cut list is empty.</p> : null}
          {n.pieces.map((piece) => (
            <figure key={piece.index} className="nest-piece" data-piece={piece.index} data-kind={piece.kind}>
              <figcaption>
                <b>{pieceNumber(piece)}</b> <span className="stock">{piece.stock}</span>
                {piece.material ? <span className="mat"> · {piece.material}</span> : null}
                <small>
                  {pieceYield(piece, r)}
                  {piece.guillotine ? "" : " · not all straight cuts"}
                </small>
              </figcaption>
              <PieceDiagram piece={piece} rounding={r} lit={p.lit} onHover={p.onHover} onSelect={p.onSelect} />
              <details>
                <summary>
                  {piece.cuts.length} cut{piece.cuts.length === 1 ? "" : "s"}, {piece.placements.length} part{piece.placements.length === 1 ? "" : "s"}
                  {p.onSvg ? (
                    <button
                      type="button"
                      className="linkish"
                      onClick={(e) => {
                        e.preventDefault();
                        p.onSvg?.(piece);
                      }}
                      title="Save this piece's cutting diagram as an SVG drawing (1:1 in mm)"
                    >
                      SVG
                    </button>
                  ) : null}
                </summary>
                <ol className="nest-cuts">
                  {piece.cuts.map((c) => (
                    <li key={c.n} value={c.n}>
                      {cutText(c, r)}
                    </li>
                  ))}
                </ol>
                <ul className="nest-parts">
                  {piece.placements.map((q) => (
                    <li key={q.tag}>
                      <span className="tag">{q.tag}</span> {q.name} {dims(q.size[1], q.size[2], r)}
                      {q.turned ? " (turned)" : ""}
                    </li>
                  ))}
                </ul>
              </details>
            </figure>
          ))}
        </div>
      )}
      {p.onSvg && p.onDxf && p.onPdf ? (
        <div className="btnrow">
          <button type="button" className="btn" disabled={!n || !n.pieces.length} onClick={() => p.onSvg?.(null)} title="Save every piece's cutting diagram as an SVG drawing (one file each, 1:1 in mm)">
            SVG
          </button>
          <button type="button" className="btn" disabled={!n || !n.pieces.length} onClick={() => p.onDxf?.("lwpolyline")} title="Save the layouts as a DXF for CNC and laser: a polyline per part, a layer per sheet or board (mm)">
            DXF
          </button>
          <button type="button" className="btn" disabled={!n || !n.pieces.length} onClick={() => p.onDxf?.("r12")} title="The same DXF in strict R12 form (POLYLINE) for older CAM programs">
            DXF R12
          </button>
          <button type="button" className="btn" disabled={!n} onClick={() => p.onPdf?.()} title="Save a PDF booklet: what to buy, then a page per sheet or board with its diagram and cut sequence">
            PDF
          </button>
        </div>
      ) : null}
      {n ? (
        <p className="note" data-testid="nest-note">
          {nestNote(n, r)}
        </p>
      ) : null}
    </section>
  );
}
