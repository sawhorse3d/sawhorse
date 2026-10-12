// The tool rail in Sheets mode (lane 12): select, and one tool per thing a sheet holds.
// Our own glyphs, drawn for this rail in the model rail's 24x24 stroke style.
import type { ReactElement } from "react";
import type { SheetToolId } from "./SheetCanvas";

export const SHEET_TOOLS: { id: SheetToolId; label: string; title: string; glyph: ReactElement }[] = [
  { id: "select", label: "Select", title: "Select, move (drag), resize (corner handles); Alt+drag slides a viewport's drawing", glyph: <path d="M5 3l14 9-6 1-3 6z" /> },
  {
    id: "viewport",
    label: "View",
    title: "Viewport: a scene or a standard view of the model, to scale",
    glyph: (
      <>
        <rect x="3" y="5" width="18" height="14" />
        <path d="M7 15l4-5 3 3 2-2 3 4" />
      </>
    ),
  },
  {
    id: "dimension",
    label: "Dim",
    title: "Dimension: click points of a viewport's drawing, then where the line goes",
    glyph: (
      <>
        <path d="M4 7v10M20 7v10M4 12h16" />
        <path d="M4 12l3-2M4 12l3 2M20 12l-3-2M20 12l-3 2" />
      </>
    ),
  },
  {
    id: "label",
    label: "Label",
    title: "Label: what it points at, then where the text goes, then type",
    glyph: (
      <>
        <path d="M4 20l7-7" />
        <rect x="11" y="5" width="10" height="8" />
        <path d="M13 8h6M13 10.5h4" />
      </>
    ),
  },
  {
    id: "table",
    label: "Table",
    title: "Cut-list table: the model's cut list, live",
    glyph: (
      <>
        <rect x="3" y="5" width="18" height="14" />
        <path d="M3 9h18M3 13h18M9 5v14" />
      </>
    ),
  },
  {
    id: "image",
    label: "Image",
    title: "Image: a PNG or JPEG placed in a frame (a logo, a photo of the piece)",
    glyph: (
      <>
        <rect x="3" y="5" width="18" height="14" />
        <circle cx="9" cy="10" r="1.6" />
        <path d="M5 17l5-4 3 2 3-3 3 3" />
      </>
    ),
  },
  { id: "line", label: "Line", title: "Line on the paper (Shift: square to the page)", glyph: <path d="M4 19L20 5" /> },
  { id: "rect", label: "Rect", title: "Rectangle on the paper", glyph: <rect x="4" y="7" width="16" height="10" /> },
];

export function SheetRail({ active, onSelect }: { active: SheetToolId; onSelect: (id: SheetToolId) => void }) {
  return (
    <aside className="rail sheet-rail" aria-label="Sheet tools">
      <div className="grp">
        {SHEET_TOOLS.map((t) => (
          <button key={t.id} type="button" className="tool" data-sheet-tool={t.id} aria-pressed={active === t.id} title={t.title} onClick={() => onSelect(t.id)}>
            <svg viewBox="0 0 24 24" aria-hidden="true">
              {t.glyph}
            </svg>
            <small>{t.label}</small>
          </button>
        ))}
      </div>
    </aside>
  );
}
