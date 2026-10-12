// The Cut list pane (lane 7), laid out as the approved mock: a table of parts (Part, Qty,
// T, W, L, Material), two summary tiles (board feet; 4 x 8 sheets), the CSV / PDF / Labels
// buttons, and a note. Added around the mock's table: the rounding and "visible only"
// controls, Print, a grain mark beside the size the grain runs along, and notes under a
// part's name (shaped, handed, hidden, sheet). A click on a row selects its pieces in the
// model; right-click sets the grain or the stock. Nesting is not built: its button says so.
import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import type { CutList, CutRow, Grain, Rounding } from "../kernel/types";
import { formatSteps, listStyle, materialText, rowNotes, ROUNDINGS, scopeNote, sizeUnit, summary, type RoundingChoice } from "../cutlist/format";

export interface CutListPaneProps {
  list: CutList | null;
  /** Why there is no list (the kernel is not ready, or it refused). */
  problem?: string | null;
  /** The Round to choice ("follow": follow the display units). */
  rounding: RoundingChoice;
  /** What "Follow units" comes to under the current units (shown in its label). */
  follows?: Rounding;
  onRounding: (r: RoundingChoice) => void;
  /** The choice is kept for next time (otherwise it lasts for this session). */
  pinned?: boolean;
  onPin?: (pinned: boolean) => void;
  visibleOnly: boolean;
  onVisibleOnly: (v: boolean) => void;
  /** Row keys whose pieces are all selected. */
  selected: Set<string>;
  onSelectRow: (row: CutRow, mode: "replace" | "add" | "toggle") => void;
  onGrain: (row: CutRow, grain: Grain) => void;
  onStock: (row: CutRow, stock: string | null, sheet: boolean | null) => void;
  onCsv: () => void;
  onPdf: () => void;
  onPrint: () => void;
  onLabels: () => void;
}

/** A stable key for a row (its part and its size). */
export function rowKey(row: CutRow): string {
  return `${JSON.stringify(row.key)}:${row.steps.join("x")}`;
}

interface Menu {
  x: number;
  y: number;
  row: CutRow;
}

/** Widest a size column grows (characters); longer sizes end in an ellipsis, in full in
 * their tooltip. Sixteenths up to 99 in fit ("99 15/16", 8; lengths to "999 15/16"), and
 * metres of mm ("2438.5"). */
const SIZE_CAP: [number, number, number] = [7, 8, 9];
/** A numeric cell's side padding (6 + 3 px) plus a pixel, and the grain mark beside a size (px). */
const PAD = 10;
const MARK = 11;

/** Fixed column widths for the pane (`table-layout: fixed`): Qty and the sizes as wide as
 * their longest value in this list (in `ch`, the table's monospace digits), capped; Part
 * and Material share what is left and end in an ellipsis. So the table never scrolls
 * sideways in the 300 px pane, at any rounding, units or grain. */
export function columnWidths(list: CutList, style: ReturnType<typeof listStyle>): string[] {
  const r = list.rounding;
  // Qty: three digits ("QTY" is as wide); a thousand pieces would end in an ellipsis.
  const qty = 3;
  const sizes = [0, 1, 2].map((k) => Math.min(SIZE_CAP[k], Math.max(1, ...list.rows.map((x) => formatSteps(x.steps[k], r, style).length))));
  const mark = (k: number) => (k === 1 && list.rows.some((x) => x.grain === "width")) || (k === 2 && list.rows.some((x) => x.grain === "length")) ? MARK : 0;
  return [`calc(${qty}ch + ${PAD}px)`, ...sizes.map((n, k) => `calc(${n}ch + ${PAD + mark(k)}px)`)];
}

const GRAIN_CHOICES: { grain: Grain; label: string }[] = [
  { grain: "length", label: "Grain along the length" },
  { grain: "width", label: "Grain across (along the width)" },
  { grain: "none", label: "No grain (MDF, painted)" },
];

export function CutListPane(p: CutListPaneProps) {
  const { list } = p;
  const [menu, setMenu] = useState<Menu | null>(null);
  const [stockFor, setStockFor] = useState<CutRow | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!menu) return;
    const close = (e: Event) => {
      if (menuRef.current && e.target instanceof Node && menuRef.current.contains(e.target)) return;
      setMenu(null);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setMenu(null);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [menu]);

  const r: Rounding = list?.rounding ?? (p.rounding === "follow" ? (p.follows ?? "sixteenth") : p.rounding);
  const style = list ? listStyle(list) : "fraction";
  const s = list ? summary(list) : null;
  const empty = !!list && list.rows.length === 0;
  const openMenu = (e: ReactMouseEvent, row: CutRow) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, row });
  };

  return (
    <div className="cutlist" data-testid="cutlist">
      <div className="cut-controls">
        <label title="How finished sizes are rounded">
          <span>Round to</span>
          <select aria-label="Round sizes to" value={p.rounding} onChange={(e) => p.onRounding(e.currentTarget.value as RoundingChoice)}>
            {ROUNDINGS.map((x) => (
              <option key={x.id} value={x.id} title={x.title}>
                {x.id === "follow" && p.follows ? `${x.label} (${ROUNDINGS.find((y) => y.id === p.follows)?.label})` : x.label}
              </option>
            ))}
          </select>
        </label>
        {p.onPin ? (
          <button
            type="button"
            className={`pin${p.pinned ? " on" : ""}`}
            aria-pressed={!!p.pinned}
            disabled={p.rounding === "follow" && !p.pinned}
            onClick={() => p.onPin?.(!p.pinned)}
            title={p.pinned ? "Kept for next time. Click to keep it for this session only." : "Keep this rounding next time (otherwise it lasts for this session)"}
          >
            Pin
          </button>
        ) : null}
        <label className="check" title="Leave out hidden items, hidden tags and hidden groups">
          <input type="checkbox" checked={p.visibleOnly} onChange={(e) => p.onVisibleOnly(e.currentTarget.checked)} />
          <span>Visible only</span>
        </label>
      </div>
      {p.problem ? <p className="note err">{p.problem}</p> : null}
      {!list ? (
        p.problem ? null : <p className="note">Working out the cut list…</p>
      ) : empty ? (
        <p className="placeholder" data-testid="cutlist-empty">
          {list.visible_only && list.totals.hidden ? "Everything is hidden. Untick Visible only to list the hidden parts too." : "No parts yet. Every solid is a part; make the parts you repeat components (select, then Make Component) and each becomes one row with its quantity."}
        </p>
      ) : (
        <>
          <div className="tablewrap">
            <table aria-label="Cut list" className="fit">
              <colgroup>
                <col className="c-part" />
                {columnWidths(list, style).map((w, k) => (
                  <col key={k} style={{ width: w }} />
                ))}
                <col className="c-mat" />
              </colgroup>
              <thead>
                <tr>
                  <th>Part</th>
                  <th className="n">Qty</th>
                  <th className="n">T</th>
                  <th className="n">W</th>
                  <th className="n">L</th>
                  <th>Material</th>
                </tr>
              </thead>
              <tbody>
                {list.rows.map((row) => {
                  const k = rowKey(row);
                  const notes = rowNotes(row);
                  if (row.sheet) notes.unshift("sheet");
                  const sel = p.selected.has(k);
                  const mark = (axis: 1 | 2) => (row.grain === (axis === 2 ? "length" : "width") ? <i className="grain" title={`Grain runs along ${axis === 2 ? "the length" : "the width"}`} aria-label="grain" /> : null);
                  return (
                    <tr
                      key={k}
                      data-row={k}
                      data-part={row.name}
                      aria-selected={sel}
                      className={`${sel ? "sel" : ""}${row.hidden === row.total && row.total ? " dim" : ""}${menu && rowKey(menu.row) === k ? " menu" : ""}`}
                      onClick={(e) => p.onSelectRow(row, e.ctrlKey || e.metaKey ? "toggle" : e.shiftKey ? "add" : "replace")}
                      onContextMenu={(e) => openMenu(e, row)}
                      title="Click to select its pieces in the model. Right-click to set the grain or the stock."
                    >
                      <td className="part" title={notes.length ? `${row.name} (${notes.join(", ")})` : row.name}>
                        <span className="name">{row.name}</span>
                        {notes.length ? <small>{notes.join(" · ")}</small> : null}
                      </td>
                      <td className="n">{row.qty}</td>
                      {[0, 1, 2].map((k) => {
                        const t = formatSteps(row.steps[k], r, style);
                        const m = k ? mark(k as 1 | 2) : null;
                        return (
                          <td key={k} className={m ? "n mk" : "n"} title={t.length > SIZE_CAP[k] ? `${t} ${sizeUnit(r)}` : undefined}>
                            <span className="v">{t}</span>
                            {m}
                          </td>
                        );
                      })}
                      <td className={`mat${row.stock_set ? " set" : ""}`} title={row.material ?? "No material or stock set"}>
                        {materialText(row)}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
          <div className="sum">
            <div data-testid="tile-board-feet">
              <b>{s!.boardFeet}</b>
              <span>{s!.boardFeetLabel}</span>
            </div>
            <div data-testid="tile-sheets">
              <b>{s!.sheets}</b>
              <span>{s!.sheetsLabel}</span>
            </div>
          </div>
        </>
      )}
      <div className="btnrow">
        <button type="button" className="btn" disabled={!list || empty} onClick={p.onCsv} title="Save the cut list as a spreadsheet (CSV)">
          CSV
        </button>
        <button type="button" className="btn" disabled={!list || empty} onClick={p.onPdf} title="Save the cut list report as a PDF">
          PDF
        </button>
        <button type="button" className="btn" disabled={!list || empty} onClick={p.onPrint} title="Print the cut list report">
          Print
        </button>
        <button type="button" className="btn" disabled={!list || empty} onClick={p.onLabels} title="Save a sheet of part labels (Avery 5160 size) as a PDF">
          Labels
        </button>
        <button type="button" className="btn" disabled title="Nesting and cutting diagrams arrive in a later lane">
          Nesting ›
        </button>
      </div>
      <p className="note" data-testid="cutlist-note">
        {scopeNote(r)} Sizes in {sizeUnit(r)}.
      </p>
      {menu ? (
        <div className="ctxmenu" ref={menuRef} role="menu" aria-label={`Cut list: ${menu.row.name}`} style={{ left: menu.x, top: menu.y }}>
          <div className="ctxtitle" title={menu.row.name}>
            {menu.row.name}
          </div>
          {GRAIN_CHOICES.map((g) => (
            <button
              key={g.grain}
              type="button"
              role="menuitemradio"
              aria-checked={menu.row.grain === g.grain}
              onClick={() => {
                setMenu(null);
                p.onGrain(menu.row, g.grain);
              }}
            >
              {g.label}
              {menu.row.grain === g.grain ? <kbd>✓</kbd> : null}
            </button>
          ))}
          <hr />
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setMenu(null);
              setStockFor(menu.row);
            }}
          >
            Stock / material…
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setMenu(null);
              p.onStock(menu.row, menu.row.stock_set ? menu.row.material : null, !menu.row.sheet);
            }}
          >
            {menu.row.sheet ? "Mark as solid lumber" : "Mark as sheet goods"}
          </button>
          <button
            type="button"
            role="menuitem"
            disabled={!menu.row.stock_set && !menu.row.sheet_set}
            onClick={() => {
              setMenu(null);
              p.onStock(menu.row, null, null);
            }}
          >
            Use the material's name
          </button>
        </div>
      ) : null}
      {stockFor ? <StockDialog row={stockFor} onClose={() => setStockFor(null)} onOk={(stock, sheet) => p.onStock(stockFor, stock, sheet)} /> : null}
    </div>
  );
}

function StockDialog({ row, onClose, onOk }: { row: CutRow; onClose: () => void; onOk: (stock: string | null, sheet: boolean | null) => void }) {
  const [v, setV] = useState(row.stock_set ? (row.material ?? "") : (row.material ?? ""));
  const [kind, setKind] = useState<"auto" | "lumber" | "sheet">(row.sheet_set ? (row.sheet ? "sheet" : "lumber") : "auto");
  const ok = () => {
    onClose();
    const text = v.trim();
    onOk(text ? text : null, kind === "auto" ? null : kind === "sheet");
  };
  return (
    <section className="name-dialog" role="dialog" aria-label={`Stock for ${row.name}`}>
      <header>
        <b>Stock for {row.name}</b>
      </header>
      <label>
        <span>Species or sheet stock (shown in the Material column)</span>
        <input
          autoFocus
          value={v}
          maxLength={64}
          placeholder="White oak, 1/4 birch ply…"
          onFocus={(e) => e.currentTarget.select()}
          onChange={(e) => setV(e.currentTarget.value)}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") ok();
            if (e.key === "Escape") onClose();
          }}
        />
      </label>
      <label>
        <span>Counted as</span>
        <select aria-label="Counted as" value={kind} onChange={(e) => setKind(e.currentTarget.value as "auto" | "lumber" | "sheet")}>
          <option value="auto">Decide from the name and thickness</option>
          <option value="lumber">Solid lumber (board feet)</option>
          <option value="sheet">Sheet goods (4×8 sheets)</option>
        </select>
      </label>
      <div className="btnrow">
        <button type="button" className="btn primary" onClick={ok}>
          Set stock
        </button>
        <button type="button" className="btn" onClick={onClose}>
          Cancel
        </button>
      </div>
    </section>
  );
}
