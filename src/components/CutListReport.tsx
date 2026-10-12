// The printable cut list (lane 7): what Print sends to the printer. It lives outside the
// app's layout (a portal on <body>), hidden on screen; while printing, the print stylesheet
// hides the app and shows only this: a title, the summary, the full table (the same columns
// and cells as the PDF: board feet for lumber, area for sheets, grain, notes), the totals,
// and the scope note. Plain black on white, no app chrome.
import { createPortal } from "react-dom";
import type { CutList } from "../kernel/types";
import { formatArea, formatBoardFeet, formatSteps, listStyle, REPORT_COLUMNS, reportCells, scopeNote, sizeUnit, summary } from "../cutlist/format";

/** Right-aligned columns: Qty, T, W, L, Bd ft / area (as in the PDF). */
const NUMERIC = new Set([1, 2, 3, 4, 7]);

export function CutListReport({ list, title }: { list: CutList | null; title: string }) {
  if (!list || typeof document === "undefined") return null;
  const r = list.rounding;
  const style = listStyle(list);
  const s = summary(list);
  return createPortal(
    <article className="print-report" aria-hidden="true" data-testid="print-report">
      <header>
        <h1>Cut list: {title}</h1>
        <p>
          {list.totals.parts} parts, {list.totals.pieces} pieces · {s.boardFeet} {s.boardFeetLabel} · {s.sheets} {s.sheetsLabel} · sizes in {sizeUnit(r)}
          {list.visible_only ? " · visible parts only" : ""}
        </p>
      </header>
      <table>
        <thead>
          <tr>
            {REPORT_COLUMNS.map((c, k) => (
              <th key={c} className={NUMERIC.has(k) ? "n" : undefined}>
                {c}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {list.rows.map((row) => (
            <tr key={`${JSON.stringify(row.key)}:${row.steps.join("x")}`}>
              {reportCells(row, list).map((c, k) => (
                <td key={k} className={NUMERIC.has(k) ? "n" : undefined}>
                  {c}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      <section className="totals">
        <h2>Totals</h2>
        <ul>
          {list.totals.lumber.map((l) => (
            <li key={`l:${l.material}`}>
              {formatBoardFeet(l.board_feet)} board feet, {l.material ?? "material not set"}
            </li>
          ))}
          {list.totals.sheets.map((x) => (
            <li key={`s:${x.material}:${x.steps}`}>
              {x.sheets} sheet{x.sheets === 1 ? "" : "s"} 4×8, {x.material ?? "sheet stock"}, {formatSteps(x.steps, r, style)} {sizeUnit(r)} thick ({formatArea(x.area_m2, r)}
              {x.oversize ? "; a piece is larger than a sheet" : ""})
            </li>
          ))}
        </ul>
      </section>
      <footer>{scopeNote(r)} Made with Sawhorse.</footer>
    </article>,
    document.body,
  );
}
