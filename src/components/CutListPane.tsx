// The Cut list pane (lane 7), laid out as the approved mock: a table of parts (Part, Qty,
// T, W, L, Material), two summary tiles (board feet; 4 x 8 sheets), the CSV / PDF / Labels
// buttons, and a note. Added around the mock's table: the rounding and "visible only"
// controls, Print, a grain mark beside the size the grain runs along, and notes under a
// part's name (shaped, handed, hidden, sheet). A click on a row selects its pieces in the
// model (Shift picks a range of rows, Ctrl toggles one); right-click sets the grain or the
// stock of the row, or of every picked row at once. The Nesting button opens the Nesting
// section under the list (lane 9: cutting diagrams; `NestingSection.tsx`).
import { useEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import type { CutList, CutRow, Grain, Rounding } from "../kernel/types";
import { BOARD_FEET_NOTE, formatSteps, geometryNote, listStyle, materialText, nominalText, rowNotes, ROUNDINGS, scopeNote, sizeUnit, summary, type RoundingChoice } from "../cutlist/format";

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
  /** Row keys whose pieces are all selected (the picked rows). */
  selected: Set<string>;
  /** Pick these rows (select their pieces); none clears the selection. */
  onSelectRows: (rows: CutRow[]) => void;
  /** Set the grain of these parts (one undo step). */
  onGrain: (rows: CutRow[], grain: Grain) => void;
  /** Set the stock of these parts (one undo step): text, null for the material's name, or
   * "keep" for each part's own; `sheet` null means decide from the name and thickness. */
  onStock: (rows: CutRow[], stock: string | null | "keep", sheet: boolean | null) => void;
  /** Lane 15: count these parts, or drop them from the totals and the nesting (one undo step). */
  onInclude?: (rows: CutRow[], include: boolean) => void;
  /** Lane 15: make each piece of the loose faces in the open context a part (Group loose faces). */
  onGroupLoose?: () => void;
  /** Lane 15 fixes: open the group or instance that holds faces outside parts (null: the top
   * level), so Make parts from loose faces can act on them there. */
  onOpenPlace?: (node: number | null) => void;
  onCsv: () => void;
  onPdf: () => void;
  onPrint: () => void;
  onLabels: () => void;
  /** The Nesting section is open (lane 9). */
  nestOpen?: boolean;
  onNest?: () => void;
  /** The Nesting section itself, drawn under the cut list while open. */
  nesting?: ReactNode;
}

/** A stable key for a row (its part and its size). */
export function rowKey(row: CutRow): string {
  return `${JSON.stringify(row.key)}:${row.steps.join("x")}`;
}

interface Menu {
  x: number;
  y: number;
  /** The rows it acts on: the right-clicked row, or every picked row when it is one of them. */
  rows: CutRow[];
}

/** "Side", or "3 parts". */
function partsLabel(rows: CutRow[]): string {
  return rows.length === 1 ? rows[0].name : `${rows.length} parts`;
}

/** The same value for every row, or undefined. */
function common<T>(rows: CutRow[], f: (r: CutRow) => T): T | undefined {
  return rows.length && rows.every((r) => f(r) === f(rows[0])) ? f(rows[0]) : undefined;
}

/** The rows a click picks: plain replaces, Ctrl toggles one, Shift picks the range from the
 * anchor (Ctrl+Shift adds the range). In list order. */
export function pickRows(rows: CutRow[], selected: Set<string>, clicked: CutRow, anchor: string | null, mods: { shift: boolean; toggle: boolean }): CutRow[] {
  const keys = rows.map(rowKey);
  const k = rowKey(clicked);
  const at = keys.indexOf(k);
  const from = anchor === null ? -1 : keys.indexOf(anchor);
  let next: Set<string>;
  if (mods.shift && from >= 0 && at >= 0) {
    const range = keys.slice(Math.min(from, at), Math.max(from, at) + 1);
    next = new Set(mods.toggle ? [...selected, ...range] : range);
  } else if (mods.toggle) {
    next = new Set(selected);
    if (next.has(k)) next.delete(k);
    else next.add(k);
  } else next = new Set([k]);
  return rows.filter((r) => next.has(rowKey(r)));
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

/** Rows the pane draws at first, and how many more each "Show more" adds (lane 16). */
export const CUT_ROWS_SHOWN = 500;

const GRAIN_CHOICES: { grain: Grain; label: string }[] = [
  { grain: "length", label: "Grain along the length" },
  { grain: "width", label: "Grain across (along the width)" },
  { grain: "none", label: "No grain (MDF, painted)" },
];

export function CutListPane(p: CutListPaneProps) {
  const { list } = p;
  const [menu, setMenu] = useState<Menu | null>(null);
  const [stockFor, setStockFor] = useState<CutRow[] | null>(null);
  /** Where a Shift-click range starts: the last row clicked without Shift. */
  const [anchor, setAnchor] = useState<string | null>(null);
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
    const k = rowKey(row);
    const picked = p.selected.has(k) && p.selected.size > 1 && list ? list.rows.filter((x) => p.selected.has(rowKey(x))) : [row];
    setMenu({ x: e.clientX, y: e.clientY, rows: picked });
  };
  const clickRow = (e: ReactMouseEvent, row: CutRow) => {
    if (!list) return;
    const toggle = e.ctrlKey || e.metaKey;
    p.onSelectRows(pickRows(list.rows, p.selected, row, anchor, { shift: e.shiftKey, toggle }));
    if (!e.shiftKey) setAnchor(rowKey(row));
  };
  const menuKeys = new Set(menu?.rows.map(rowKey) ?? []);
  // Lane 16: a long list draws its first rows, and more on request; the totals, the
  // exports, Print and selecting rows in the model always cover every row. (Every row is
  // drawn as part of the page, and a model of 100,000 loose solids is 100,000 rows.)
  const [shown, setShown] = useState(CUT_ROWS_SHOWN);
  const rowCount = list?.rows.length ?? 0;
  const widths = useMemo(() => (list ? columnWidths(list, style) : []), [list, style]);

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
      {list && geometryNote(list) ? (
        <p className="note geom" data-testid="cutlist-geometry">
          <span>{geometryNote(list)}.</span>
          {list.totals.loose_here > 0 && p.onGroupLoose ? (
            <button type="button" className="link" onClick={p.onGroupLoose} title="Put each connected piece of the loose faces here into a group of its own, so each becomes a part (one undo step; nothing moves)">
              Make parts from loose faces
            </button>
          ) : null}
          {list.totals.loose_here === 0 && list.totals.loose_places?.length && p.onOpenPlace ? (
            <button
              type="button"
              className="link"
              onClick={() => p.onOpenPlace?.(list.totals.loose_places[0].node)}
              title={`The faces outside parts are in ${list.totals.loose_places.map((x) => `${x.node === null ? "the top level" : x.name} (${x.faces})`).join(", ")}. Open it, then Make parts from loose faces there.`}
            >
              {list.totals.loose_places[0].node === null ? "Go to the top level" : `Open ${list.totals.loose_places[0].name}`}
            </button>
          ) : null}
        </p>
      ) : null}
      {!list ? (
        p.problem ? null : <p className="note">Working out the cut list…</p>
      ) : empty ? (
        <p className="placeholder" data-testid="cutlist-empty">
          {list.visible_only && list.totals.hidden
            ? "Everything is hidden. Untick Visible only to list the hidden parts too."
            : list.totals.loose_faces
              ? `No parts yet: the model's ${list.totals.loose_faces} face${list.totals.loose_faces === 1 ? " is" : "s are"} loose, outside any group or component, and only a solid, a group or a component is a part. Make parts from loose faces puts each connected piece into a group of its own; to list each board on its own, select a board's faces and Make Group.`
              : "No parts yet. Every solid is a part; make the parts you repeat components (select, then Make Component) and each becomes one row with its quantity."}
        </p>
      ) : (
        <>
          <div className="tablewrap">
            <table aria-label="Cut list" className="fit">
              <colgroup>
                <col className="c-part" />
                {widths.map((w, k) => (
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
                {list.rows.slice(0, shown).map((row) => {
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
                      data-not-solid={row.not_solid ? "true" : undefined}
                      data-uncounted={row.uncounted}
                      className={`${sel ? "sel" : ""}${row.hidden === row.total && row.total ? " dim" : ""}${row.included === false || row.uncounted ? " out" : ""}${menuKeys.has(k) ? " menu" : ""}`}
                      onMouseDown={(e) => e.shiftKey && e.preventDefault()}
                      onClick={(e) => clickRow(e, row)}
                      onContextMenu={(e) => openMenu(e, row)}
                      title="Click to select its pieces in the model (Shift: a range of rows, Ctrl: one more). Right-click to set the grain or the stock of the picked rows."
                    >
                      <td className="part" title={notes.length ? `${row.name} (${notes.join(", ")})` : row.name}>
                        {p.onInclude ? (
                          <input
                            type="checkbox"
                            className="inc"
                            checked={row.included !== false}
                            aria-label={`Include ${row.name}`}
                            title={row.included === false ? "Left out of the totals and the nesting. Tick to count it again." : "Counted. Untick to leave it out of the totals and the nesting (it stays listed)."}
                            onClick={(e) => e.stopPropagation()}
                            onChange={(e) => p.onInclude?.([row], e.currentTarget.checked)}
                          />
                        ) : null}
                        <span className="name">{row.name}</span>
                        {notes.length ? <small>{notes.join(" · ")}</small> : null}
                      </td>
                      <td className="n">{row.qty}</td>
                      {[0, 1, 2].map((k) => {
                        const t = formatSteps(row.steps[k], r, style);
                        const m = k ? mark(k as 1 | 2) : null;
                        return (
                          <td
                            key={k}
                            className={m ? "n mk" : "n"}
                            data-nominal={k === 0 && nominalText(row) ? row.nominal : undefined}
                            title={k === 0 && nominalText(row) ? `${t} ${sizeUnit(r)} finished; board feet on ${nominalText(row)}` : t.length > SIZE_CAP[k] ? `${t} ${sizeUnit(r)}` : undefined}
                          >
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
            {rowCount > shown ? (
              <p className="note more" data-testid="cutlist-more">
                {`${rowCount - shown} more rows not drawn here (the totals, CSV, PDF and Print include them). `}
                <button type="button" className="link" onClick={() => setShown((n) => n + CUT_ROWS_SHOWN)}>
                  {`Show ${Math.min(CUT_ROWS_SHOWN, rowCount - shown)} more`}
                </button>
              </p>
            ) : null}
          </div>
          <div className="sum">
            <div data-testid="tile-board-feet" title={BOARD_FEET_NOTE}>
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
        <button
          type="button"
          className="btn"
          aria-pressed={!!p.nestOpen}
          disabled={!p.onNest || (!p.nestOpen && (!list || empty))}
          onClick={p.onNest}
          title="Lay the parts out on sheets and boards: cutting diagrams, the cut order, SVG, DXF and a PDF booklet"
        >
          {p.nestOpen ? "Nesting ‹" : "Nesting ›"}
        </button>
      </div>
      <p className="note" data-testid="cutlist-note">
        {scopeNote(r)} Sizes in {sizeUnit(r)}.
      </p>
      {p.nesting}
      {menu ? (
        <div className="ctxmenu" ref={menuRef} role="menu" aria-label={`Cut list: ${partsLabel(menu.rows)}`} style={{ left: menu.x, top: menu.y }}>
          <div className="ctxtitle" title={menu.rows.map((r) => r.name).join(", ")}>
            {menu.rows.length === 1 ? menu.rows[0].name : `${menu.rows.length} parts: ${menu.rows.map((r) => r.name).join(", ")}`}
          </div>
          {GRAIN_CHOICES.map((g) => {
            const on = menu.rows.every((r) => r.grain === g.grain);
            return (
              <button
                key={g.grain}
                type="button"
                role="menuitemradio"
                aria-checked={on}
                onClick={() => {
                  setMenu(null);
                  p.onGrain(menu.rows, g.grain);
                }}
              >
                {g.label}
                {on ? <kbd>✓</kbd> : null}
              </button>
            );
          })}
          <hr />
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setMenu(null);
              setStockFor(menu.rows);
            }}
          >
            Stock / material…
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setMenu(null);
              p.onStock(menu.rows, "keep", !menu.rows.every((r) => r.sheet));
            }}
          >
            {menu.rows.every((r) => r.sheet) ? "Mark as solid lumber" : "Mark as sheet goods"}
          </button>
          {p.onInclude ? (
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                setMenu(null);
                p.onInclude?.(menu.rows, menu.rows.some((r) => r.included === false));
              }}
            >
              {menu.rows.some((r) => r.included === false) ? "Count in the totals" : "Leave out of the totals"}
            </button>
          ) : null}
          <button
            type="button"
            role="menuitem"
            disabled={!menu.rows.some((r) => r.stock_set || r.sheet_set)}
            onClick={() => {
              setMenu(null);
              p.onStock(menu.rows, null, null);
            }}
          >
            Use the material's name
          </button>
        </div>
      ) : null}
      {stockFor ? (
        <StockDialog
          rows={stockFor}
          material={common(stockFor, (r) => r.material) ?? null}
          kind={common(stockFor, (r) => (r.sheet_set ? (r.sheet ? "sheet" : "lumber") : "auto")) ?? "auto"}
          onClose={() => setStockFor(null)}
          onOk={(stock, sheet) => p.onStock(stockFor, stock, sheet)}
        />
      ) : null}
    </div>
  );
}

function StockDialog({ rows, material, kind: initial, onClose, onOk }: { rows: CutRow[]; material: string | null; kind: "auto" | "lumber" | "sheet"; onClose: () => void; onOk: (stock: string | null, sheet: boolean | null) => void }) {
  const [v, setV] = useState(material ?? "");
  const [kind, setKind] = useState<"auto" | "lumber" | "sheet">(initial);
  const ok = () => {
    onClose();
    const text = v.trim();
    onOk(text ? text : null, kind === "auto" ? null : kind === "sheet");
  };
  const who = partsLabel(rows);
  return (
    <section className="name-dialog" role="dialog" aria-label={`Stock for ${who}`}>
      <header>
        <b>Stock for {who}</b>
        {rows.length > 1 ? <small title={rows.map((r) => r.name).join(", ")}>{rows.map((r) => r.name).join(", ")}</small> : null}
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
