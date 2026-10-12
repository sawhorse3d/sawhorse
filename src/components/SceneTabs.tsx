// Scenes as tabs above the viewport (lane 5): a scene is a named save of the camera, which
// tags are hidden, and which group or component is open. Click a tab to go to it; "+"
// saves the current view as a new scene; right-click a tab to update it to the current
// view, rename it or delete it; double-click to rename.
import { useEffect, useRef, useState } from "react";
import type { SceneDef } from "../kernel/types";
import { PanelGlyph } from "./glyphs";

export interface SceneTabsProps {
  scenes: SceneDef[];
  /** The scene last gone to (null: none, or the view changed since). */
  active: string | null;
  onGo: (name: string) => void;
  onAdd: () => void;
  onUpdate: (name: string) => void;
  onRename: (name: string, next: string) => void;
  onDelete: (name: string) => void;
}

export function SceneTabs({ scenes, active, onGo, onAdd, onUpdate, onRename, onDelete }: SceneTabsProps) {
  const [menu, setMenu] = useState<{ name: string; x: number; y: number } | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
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

  return (
    <nav className="scenes" aria-label="Scenes">
      <div role="tablist" aria-label="Scenes" className="scene-list">
        {scenes.map((s) =>
          renaming === s.name ? (
            <input
              key={s.name}
              className="rename"
              aria-label={`New name for scene ${s.name}`}
              autoFocus
              defaultValue={s.name}
              onFocus={(e) => e.currentTarget.select()}
              onKeyDown={(e) => {
                e.stopPropagation();
                if (e.key === "Enter") {
                  const v = e.currentTarget.value.trim();
                  setRenaming(null);
                  if (v && v !== s.name) onRename(s.name, v);
                } else if (e.key === "Escape") setRenaming(null);
              }}
              onBlur={() => setRenaming(null)}
            />
          ) : (
            <button
              key={s.name}
              type="button"
              role="tab"
              aria-selected={active === s.name}
              data-scene={s.name}
              title={`${s.name}: ${s.parallel ? "parallel" : "perspective"}${s.hidden_tags?.length ? `, hides ${s.hidden_tags.join(", ")}` : ""}. Right-click to update, rename or delete.`}
              onClick={() => onGo(s.name)}
              onDoubleClick={() => setRenaming(s.name)}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ name: s.name, x: e.clientX, y: e.clientY });
              }}
            >
              {s.name}
            </button>
          ),
        )}
      </div>
      <button type="button" className="scene-add" aria-label="Add a scene from the current view" title="Save the current view (camera, hidden tags, open group) as a scene" onClick={() => onAdd()}>
        <PanelGlyph name="plus" />
        {scenes.length ? null : <span>Scene</span>}
      </button>
      {menu ? (
        <div className="ctxmenu" ref={ref} role="menu" aria-label={`Scene ${menu.name}`} style={{ left: menu.x, top: menu.y }}>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setMenu(null);
              onUpdate(menu.name);
            }}
          >
            Update to this view
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setMenu(null);
              setRenaming(menu.name);
            }}
          >
            Rename
          </button>
          <hr />
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setMenu(null);
              onDelete(menu.name);
            }}
          >
            Delete
          </button>
        </div>
      ) : null}
    </nav>
  );
}
