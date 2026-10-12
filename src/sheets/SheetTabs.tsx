// Model / Sheets (lane 12): the switch at the start of the tab row, and in Sheets mode the
// sheets as tabs (click to show, double-click to rename, right-click to move or delete,
// "+" to add one), in the scene tabs' row and style.
import { useEffect, useRef, useState } from "react";
import type { SheetRow } from "../kernel/sheetTypes";
import { PanelGlyph } from "../components/glyphs";

export type WorkMode = "model" | "sheets";

export function ModeSwitch({ mode, onMode, sheets }: { mode: WorkMode; onMode: (m: WorkMode) => void; sheets: number }) {
  return (
    <div className="mode-switch" role="radiogroup" aria-label="Model or sheets">
      <button type="button" role="radio" aria-checked={mode === "model"} data-mode="model" onClick={() => onMode("model")} title="The 3D model">
        Model
      </button>
      <button type="button" role="radio" aria-checked={mode === "sheets"} data-mode="sheets" onClick={() => onMode("sheets")} title="Drawing sheets: scaled views of the model on paper, dimensioned, with a title block">
        Sheets{sheets ? <small>{sheets}</small> : null}
      </button>
    </div>
  );
}

export interface SheetTabsProps {
  lead: React.ReactNode;
  sheets: SheetRow[];
  active: number | null;
  onGo: (id: number) => void;
  onAdd: () => void;
  onRename: (id: number, name: string) => void;
  onMove: (id: number, to: number) => void;
  onDelete: (id: number) => void;
}

export function SheetTabs({ lead, sheets, active, onGo, onAdd, onRename, onMove, onDelete }: SheetTabsProps) {
  const [menu, setMenu] = useState<{ id: number; x: number; y: number } | null>(null);
  const [renaming, setRenaming] = useState<number | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!menu) return;
    const close = (e: Event) => {
      if (ref.current && e.target instanceof Node && ref.current.contains(e.target)) return;
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
  const index = (id: number) => sheets.findIndex((s) => s.id === id);

  return (
    <nav className="scenes sheet-tabs" aria-label="Sheets">
      {lead}
      <div role="tablist" aria-label="Sheets" className="scene-list">
        {sheets.map((s) =>
          renaming === s.id ? (
            <input
              key={s.id}
              className="rename"
              aria-label={`New name for sheet ${s.sheet.name}`}
              autoFocus
              defaultValue={s.sheet.name}
              maxLength={64}
              onFocus={(e) => e.currentTarget.select()}
              onKeyDown={(e) => {
                e.stopPropagation();
                if (e.key === "Enter") {
                  const v = e.currentTarget.value.trim();
                  setRenaming(null);
                  if (v && v !== s.sheet.name) onRename(s.id, v);
                } else if (e.key === "Escape") setRenaming(null);
              }}
              onBlur={() => setRenaming(null)}
            />
          ) : (
            <button
              key={s.id}
              type="button"
              role="tab"
              aria-selected={active === s.id}
              data-sheet={s.sheet.name}
              title={`${s.sheet.name}: ${s.sheet.items.length} item${s.sheet.items.length === 1 ? "" : "s"}. Double-click to rename; right-click to move or delete.`}
              onClick={() => onGo(s.id)}
              onDoubleClick={() => setRenaming(s.id)}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ id: s.id, x: e.clientX, y: e.clientY });
              }}
            >
              {s.sheet.name}
            </button>
          ),
        )}
      </div>
      <button type="button" className="scene-add" aria-label="Add a sheet" title="Add a sheet (Letter landscape, title block on)" onClick={() => onAdd()}>
        <PanelGlyph name="plus" />
        {sheets.length ? null : <span>Sheet</span>}
      </button>
      {menu ? (
        <div className="ctxmenu" ref={ref} role="menu" aria-label="Sheet" style={{ left: menu.x, top: menu.y }}>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setMenu(null);
              setRenaming(menu.id);
            }}
          >
            Rename
          </button>
          <button
            type="button"
            role="menuitem"
            disabled={index(menu.id) <= 0}
            onClick={() => {
              setMenu(null);
              onMove(menu.id, index(menu.id) - 1);
            }}
          >
            Move left
          </button>
          <button
            type="button"
            role="menuitem"
            disabled={index(menu.id) >= sheets.length - 1}
            onClick={() => {
              setMenu(null);
              onMove(menu.id, index(menu.id) + 1);
            }}
          >
            Move right
          </button>
          <hr />
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setMenu(null);
              onDelete(menu.id);
            }}
          >
            Delete
          </button>
        </div>
      ) : null}
    </nav>
  );
}
