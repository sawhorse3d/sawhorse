// The Outliner (lane 5): the model's tree of groups, component instances and solids, laid
// out like the approved mock's Outliner (indented rows, a muted kind hint on the right of
// each label). Each row has an eye (hide / show) and a lock; a click selects (Shift adds,
// Ctrl toggles) and the viewport's selection shows here; a double-click on a name renames
// it; right-click offers Make Group, Make Component, Make Unique, Explode, Hide, Lock,
// Rename and Delete. The open group or component is marked "editing". Section planes (lane
// 12) are listed in their group or component; right-click activates, reverses, slices or
// exports one.
import { useEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import type { DocState, Entity } from "../kernel/types";
import { selectionLookup } from "../lib/entities";
import { chain, graphOf } from "../lib/scope";
import { PanelGlyph } from "./glyphs";

export type OutlinerAction =
  | "edit"
  | "group"
  | "component"
  | "unique"
  | "explode"
  | "hide"
  | "show"
  | "lock"
  | "unlock"
  | "delete"
  // Section planes (lane 12).
  | "activate"
  | "deactivate"
  | "reverse"
  | "slice"
  | "export-section-svg"
  | "export-section-dxf";

export interface OutlineRow {
  key: string;
  /** What selecting the row selects (imported meshes are listed, not selectable). */
  entity: Entity | null;
  kind: "group" | "instance" | "solid" | "mesh" | "section";
  label: string;
  /** Muted hint: "component · 2 of 4", "group", "not a solid". */
  hint: string;
  level: number;
  hidden: boolean;
  locked: boolean;
  /** Drawn (not hidden itself, nor by a tag or a hidden ancestor). */
  visible: boolean;
  parent: number | null;
  /** Node rows: has anything inside. */
  children: boolean;
  /** On the open context's path. */
  open: boolean;
  /** The definition (instances). */
  definition: number | null;
  /** How many instances its definition has (instances). */
  instances: number;
  /** Section planes: the active one of its context. */
  active?: boolean;
}

/** The tree as rows, depth first: in each container its groups and instances (by id), then
 * its solids (in creation order), then its imported meshes. `expanded`: node ids whose
 * contents are listed. */
export function outlineRows(state: DocState | null, expanded: Set<number>): OutlineRow[] {
  const g = graphOf(state);
  const rows: OutlineRow[] = [];
  const names = new Map((state?.object_info ?? []).map((o) => [o.id, o.name]));
  const tags = new Map((state?.object_info ?? []).map((o) => [o.id, o.tag]));
  const numbers = new Map((state?.solids ?? []).map((s) => [s.id, s.number]));
  const meshNames = new Map((state?.meshes ?? []).map((m) => [m.id, m.name]));
  const definitions = new Map(g.definitions.map((d) => [d.id, d]));
  const path = new Set(g.path);
  const counter = new Map<number, number>();
  // Lane 16: what each container holds, indexed once (in the graph's order). The walk used
  // to filter every list for every container, which at the model's 100,000 solids was 20 s
  // of the window's thread.
  const by = <T extends { parent: number | null }>(list: readonly T[]) => {
    const m = new Map<number | null, T[]>();
    for (const x of list) {
      const at = m.get(x.parent);
      if (at) at.push(x);
      else m.set(x.parent, [x]);
    }
    return m;
  };
  const nodesIn = by(g.nodes);
  const objectsIn = by(g.objects);
  const meshesIn = by(g.meshes);
  const sectionsIn = by(g.sections ?? []);
  const holds = (id: number) => nodesIn.has(id) || objectsIn.has(id) || meshesIn.has(id) || sectionsIn.has(id);
  const walk = (parent: number | null, level: number) => {
    for (const n of nodesIn.get(parent) ?? []) {
      const def = n.definition !== null ? definitions.get(n.definition) : undefined;
      let hint = "group";
      if (n.kind === "instance") {
        const k = (counter.get(n.definition ?? 0) ?? 0) + 1;
        counter.set(n.definition ?? 0, k);
        hint = def && def.instances > 1 ? `component · ${k} of ${def.instances}` : "component";
      }
      if (n.hidden) hint += " · hidden";
      if (n.tag) hint += ` · ${n.tag}`;
      rows.push({
        key: `n:${n.id}`,
        entity: { kind: "node", node: n.id },
        kind: n.kind,
        label: n.label,
        hint,
        level,
        hidden: n.hidden,
        locked: n.locked,
        visible: n.visible,
        parent: n.parent,
        children: holds(n.id),
        open: path.has(n.id),
        definition: n.definition,
        instances: def?.instances ?? 0,
      });
      if (expanded.has(n.id)) walk(n.id, level + 1);
    }
    for (const o of objectsIn.get(parent) ?? []) {
      const tag = tags.get(o.id);
      rows.push({
        key: `o:${o.id}`,
        entity: { kind: "object", object: o.id },
        kind: "solid",
        label: names.get(o.id) ?? `Solid ${numbers.get(o.id) ?? ""}`.trim(),
        hint: [o.hidden ? "hidden" : "", tag ?? ""].filter(Boolean).join(" · "),
        level,
        hidden: o.hidden,
        locked: o.locked,
        visible: o.visible,
        parent: o.parent,
        children: false,
        open: false,
        definition: null,
        instances: 0,
      });
    }
    for (const m of meshesIn.get(parent) ?? []) {
      rows.push({
        key: `m:${m.id}`,
        entity: null,
        kind: "mesh",
        label: meshNames.get(m.id) ?? "Mesh",
        hint: "not a solid",
        level,
        hidden: m.hidden,
        locked: m.locked,
        visible: m.visible,
        parent: m.parent,
        children: false,
        open: false,
        definition: null,
        instances: 0,
      });
    }
    for (const s of sectionsIn.get(parent) ?? []) {
      rows.push({
        key: `s:${s.id}`,
        entity: { kind: "section", section: s.id },
        kind: "section",
        label: s.name,
        hint: ["section", s.active ? "active" : "", s.hidden ? "hidden" : ""].filter(Boolean).join(" · "),
        level,
        hidden: s.hidden,
        locked: false,
        visible: s.visible,
        parent: s.parent,
        children: false,
        open: false,
        definition: null,
        instances: 0,
        active: s.active,
      });
    }
  };
  walk(null, 0);
  return rows;
}

/** Rows above which the Outliner draws only those in view (lane 16). */
export const OUTLINE_WINDOW_FROM = 300;
/** A row's height when the Outliner draws only those in view (`.tree.outline.windowed li`). */
export const OUTLINE_ROW_PX = 24;

/** The rows to draw of `count`, for a list scrolled `top` px past its first row in a view
 * `height` px tall: those in view and a screenful either side (a view of no height, as in
 * a hidden pane, is taken as a tall one). */
export function outlineWindow(count: number, top: number, height: number): { first: number; last: number } {
  const h = height > 0 ? height : 1200;
  const first = Math.max(0, Math.floor((top - h) / OUTLINE_ROW_PX));
  const last = Math.min(count, Math.max(first, Math.ceil((top + 2 * h) / OUTLINE_ROW_PX)));
  return { first: Math.min(first, count), last };
}

export interface OutlinerProps {
  state: DocState | null;
  selection: Entity[];
  /** A row was clicked (mode from Shift / Ctrl). */
  onSelect: (row: OutlineRow, mode: "replace" | "add" | "toggle") => void;
  /** A context-menu, eye or lock action on these rows. */
  onAction: (action: OutlinerAction, rows: OutlineRow[]) => void;
  onRename: (row: OutlineRow, name: string | null) => void;
  /** #117: rows dragged onto a group row (into it), onto another row (beside it, into what
   * holds it) or onto the empty space below the rows (`null`: out to the model). */
  onMove?: (rows: OutlineRow[], into: number | null) => void;
}

/** #117: a row that can be dragged: a solid, group or component, not locked. Imported meshes
 * and section planes are not items. */
export function draggableRow(r: OutlineRow): boolean {
  return !!r.entity && (r.kind === "solid" || r.kind === "group" || r.kind === "instance") && !r.locked;
}

/** #117: where a drop on `target` puts things: a group row takes them in, any other row puts
 * them beside it, in what holds it. `undefined`: not a place to drop, as on a locked row. */
export function dropInto(target: OutlineRow): number | null | undefined {
  if (target.locked) return undefined;
  if (target.kind === "group" && target.entity?.kind === "node") return target.entity.node;
  return target.parent;
}

/** How far the pointer moves, in pixels, before a press on a row becomes a drag. */
const DRAG_PX = 4;

interface Drag {
  rows: OutlineRow[];
  x: number;
  y: number;
  /** Past the threshold: a drag, not a click. */
  on: boolean;
  /** The row under the pointer while dragging. */
  over: string | null;
}

interface Menu {
  x: number;
  y: number;
  rows: OutlineRow[];
}

export function Outliner({ state, selection, onSelect, onAction, onRename, onMove }: OutlinerProps) {
  const g = graphOf(state);
  const [expanded, setExpanded] = useState<Set<number>>(() => new Set());
  // The open context's path is always unfolded.
  const pathKey = g.path.join(",");
  useEffect(() => {
    if (!g.path.length) return;
    setExpanded((s) => {
      const next = new Set(s);
      for (const n of g.path) next.add(n);
      // And what holds the open one.
      for (const n of chain(g, g.nodes.find((x) => x.id === g.context)?.parent ?? null)) next.add(n);
      return next;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pathKey]);
  const rows = useMemo(() => outlineRows(state, expanded), [state, expanded]);
  // Lane 16: a long tree draws only the rows in view (and a screenful either side); the
  // list keeps its full height, so the scroll bar and scrolling work as before. Every row
  // is drawn as a node of the page, and at the model's 100,000 solids drawing them all kept
  // the window from answering for half a minute after each edit.
  const listRef = useRef<HTMLUListElement>(null);
  const windowed = rows.length > OUTLINE_WINDOW_FROM;
  const [view, setView] = useState({ top: 0, height: 0 });
  useEffect(() => {
    const ul = listRef.current;
    if (!windowed || !ul) return;
    let scroller: HTMLElement | null = ul.parentElement;
    while (scroller && !/(auto|scroll)/.test(getComputedStyle(scroller).overflowY)) scroller = scroller.parentElement;
    const box = scroller ?? document.documentElement;
    const update = () => {
      const top = box.getBoundingClientRect().top - ul.getBoundingClientRect().top;
      const height = box.clientHeight;
      setView((v) => (Math.abs(v.top - top) < OUTLINE_ROW_PX / 2 && v.height === height ? v : { top, height }));
    };
    update();
    const target: HTMLElement | Window = scroller ?? window;
    target.addEventListener("scroll", update, { passive: true });
    const ro = typeof ResizeObserver === "function" ? new ResizeObserver(update) : null;
    ro?.observe(box);
    return () => {
      target.removeEventListener("scroll", update);
      ro?.disconnect();
    };
  }, [windowed]);
  const span = windowed ? outlineWindow(rows.length, view.top, view.height) : { first: 0, last: rows.length };
  // Lane 16b: membership by number for solids and groups, built once per selection.
  const selected = useMemo(() => selectionLookup(selection), [selection]);
  const isSelected = (r: OutlineRow) => !!r.entity && selected.has(r.entity);
  const nodeOf = (r: OutlineRow) => (r.entity?.kind === "node" ? r.entity.node : -1);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
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

  // #117: drag rows onto a group. A press on a row starts it; it becomes a drag once the
  // pointer has moved a few pixels, so a click still selects. Escape cancels; a release
  // anywhere but a row or the space below the rows drops nothing.
  const [drag, setDrag] = useState<Drag | null>(null);
  const dragRef = useRef<Drag | null>(null);
  const dragged = useRef(false);
  const setDragBoth = (d: Drag | null) => {
    dragRef.current = d;
    setDrag(d);
  };
  // The click the browser fires after the release that ends a drag selects nothing. It goes
  // to the row only when the press and the release were on the same one (to a shared parent
  // otherwise), so the mark is cleared once the release's events are done: it must never
  // swallow a later click on another row.
  const endDrag = () => {
    dragged.current = true;
    setTimeout(() => {
      dragged.current = false;
    }, 0);
  };
  useEffect(() => {
    if (!drag) return;
    const move = (e: MouseEvent) => {
      const d = dragRef.current;
      if (!d || d.on || Math.hypot(e.clientX - d.x, e.clientY - d.y) < DRAG_PX) return;
      setDragBoth({ ...d, on: true });
    };
    const up = () => {
      // A release on a row or the space below the rows is handled there first.
      if (dragRef.current?.on) endDrag();
      setDragBoth(null);
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !dragRef.current) return;
      e.stopPropagation();
      setDragBoth(null);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    window.addEventListener("keydown", esc, true);
    return () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      window.removeEventListener("keydown", esc, true);
    };
  }, [drag !== null]); // eslint-disable-line react-hooks/exhaustive-deps

  const startDrag = (e: ReactMouseEvent, row: OutlineRow) => {
    if (e.button !== 0 || !onMove || !draggableRow(row) || renaming) return;
    const rowsDragged = isSelected(row) ? rows.filter(isSelected) : [row];
    if (!rowsDragged.every(draggableRow)) return;
    dragged.current = false;
    setDragBoth({ rows: rowsDragged, x: e.clientX, y: e.clientY, on: false, over: null });
  };
  const dropOn = (into: number | null | undefined) => {
    const d = dragRef.current;
    setDragBoth(null);
    if (!d?.on) return;
    endDrag();
    if (into !== undefined) onMove?.(d.rows, into);
  };
  const dragging = !!drag?.on;
  const isDragged = (r: OutlineRow) => !!drag?.on && drag.rows.some((x) => x.key === r.key);

  const toggle = (id: number) =>
    setExpanded((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const openMenu = (e: ReactMouseEvent, row: OutlineRow) => {
    e.preventDefault();
    if (!row.entity) return;
    const inSel = isSelected(row);
    const targets = inSel ? rows.filter(isSelected) : [row];
    if (!inSel) onSelect(row, "replace");
    setMenu({ x: e.clientX, y: e.clientY, rows: targets });
  };

  const empty = rows.length === 0;
  const m = menu;
  const nodesOnly = m ? m.rows.every((r) => r.kind === "group" || r.kind === "instance") : false;
  const sectionRow = m && m.rows.length === 1 && m.rows[0].kind === "section" ? m.rows[0] : null;
  const anySection = m ? m.rows.some((r) => r.kind === "section") : false;
  const sameParent = m ? new Set(m.rows.map((r) => r.parent)).size === 1 : false;
  const solidsOrNodes = m ? m.rows.every((r) => r.kind !== "mesh" && r.kind !== "section") : false;
  const anyHidden = m ? m.rows.some((r) => r.hidden) : false;
  const anyLocked = m ? m.rows.some((r) => r.locked) : false;
  const act = (a: OutlinerAction) => {
    if (!m) return;
    setMenu(null);
    onAction(a, m.rows);
  };

  return (
    <div className="outliner" data-testid="outliner">
      {empty ? (
        <p className="note">Nothing yet. Draw a shape and push it into a solid; select solids and press Ctrl+G to group them.</p>
      ) : (
        <ul
          ref={listRef}
          className={windowed ? "tree outline windowed" : "tree outline"}
          role="tree"
          aria-label="Groups, components and solids"
          data-rows={rows.length}
          style={windowed ? { paddingTop: span.first * OUTLINE_ROW_PX, paddingBottom: (rows.length - span.last) * OUTLINE_ROW_PX } : undefined}
        >
          {rows.slice(span.first, span.last).map((r, i) => {
            const isSel = isSelected(r);
            return (
              <li
                key={r.key}
                role="treeitem"
                aria-setsize={windowed ? rows.length : undefined}
                aria-posinset={windowed ? span.first + i + 1 : undefined}
                aria-selected={isSel}
                aria-expanded={r.children ? expanded.has(nodeOf(r)) : undefined}
                data-row={r.key}
                data-kind={r.kind}
                className={`${isSel ? "sel" : ""}${r.visible ? "" : " dim"}${r.open ? " open" : ""}${r.active ? " active" : ""}${dragging && drag?.over === r.key && !isDragged(r) && dropInto(r) !== undefined ? " drop" : ""}${isDragged(r) ? " dragged" : ""}`}
                style={{ paddingLeft: 4 + r.level * 14 }}
                data-draggable={onMove && draggableRow(r) ? "true" : undefined}
                onMouseDown={(e) => startDrag(e, r)}
                onMouseEnter={() => {
                  const d = dragRef.current;
                  if (d?.on && d.over !== r.key) setDragBoth({ ...d, over: r.key });
                }}
                onMouseUp={(e) => {
                  if (!dragRef.current?.on) return;
                  e.stopPropagation();
                  dropOn(isDragged(r) ? undefined : dropInto(r));
                }}
                onClick={(e) => {
                  // The click that ends a drag selects nothing.
                  if (dragged.current) {
                    dragged.current = false;
                    return;
                  }
                  if (r.entity) onSelect(r, e.ctrlKey || e.metaKey ? "toggle" : e.shiftKey ? "add" : "replace");
                }}
                onContextMenu={(e) => openMenu(e, r)}
              >
                {r.children ? (
                  <button
                    type="button"
                    className="chev"
                    aria-label={expanded.has(nodeOf(r)) ? `Fold ${r.label}` : `Unfold ${r.label}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      toggle(nodeOf(r));
                    }}
                  >
                    <PanelGlyph name={expanded.has(nodeOf(r)) ? "open" : "closed"} />
                  </button>
                ) : (
                  <span className="chev" />
                )}
                {renaming === r.key ? (
                  <input
                    className="rename"
                    aria-label={`New name for ${r.label}`}
                    autoFocus
                    defaultValue={r.label}
                    onFocus={(e) => e.currentTarget.select()}
                    onClick={(e) => e.stopPropagation()}
                    onKeyDown={(e) => {
                      e.stopPropagation();
                      if (e.key === "Enter") {
                        const v = e.currentTarget.value.trim();
                        setRenaming(null);
                        if (v !== r.label) onRename(r, v || null);
                      } else if (e.key === "Escape") setRenaming(null);
                    }}
                    onBlur={() => setRenaming(null)}
                  />
                ) : (
                  <span
                    className="label"
                    title="Double-click to rename"
                    onDoubleClick={(e) => {
                      e.stopPropagation();
                      if (r.kind !== "mesh") setRenaming(r.key);
                    }}
                  >
                    {r.label}
                  </span>
                )}
                {r.open ? <span className="k editing">editing</span> : null}
                <span className="k">{r.hint}</span>
                <span className="row-tools">
                  <button
                    type="button"
                    aria-label={r.hidden ? `Show ${r.label}` : `Hide ${r.label}`}
                    aria-pressed={r.hidden}
                    title={r.hidden ? "Show" : "Hide"}
                    disabled={r.kind === "mesh"}
                    onClick={(e) => {
                      e.stopPropagation();
                      onAction(r.hidden ? "show" : "hide", [r]);
                    }}
                  >
                    <PanelGlyph name={r.hidden ? "eye-off" : "eye"} />
                  </button>
                  <button
                    type="button"
                    aria-label={r.locked ? `Unlock ${r.label}` : `Lock ${r.label}`}
                    aria-pressed={r.locked}
                    title={r.locked ? "Unlock" : "Lock"}
                    disabled={r.kind === "mesh" || r.kind === "section"}
                    className={r.locked ? "on" : ""}
                    onClick={(e) => {
                      e.stopPropagation();
                      onAction(r.locked ? "unlock" : "lock", [r]);
                    }}
                  >
                    <PanelGlyph name={r.locked ? "lock" : "unlock"} />
                  </button>
                </span>
              </li>
            );
          })}
        </ul>
      )}
      {dragging ? (
        <div
          className={`drop-model${drag?.over === "model" ? " drop" : ""}`}
          data-testid="outliner-drop-model"
          onMouseEnter={() => {
            const d = dragRef.current;
            if (d?.on) setDragBoth({ ...d, over: "model" });
          }}
          onMouseUp={(e) => {
            e.stopPropagation();
            dropOn(null);
          }}
        >
          Drop here to move out to the model
        </div>
      ) : null}
      {m ? (
        <div className="ctxmenu" ref={menuRef} role="menu" aria-label="Outliner actions" style={{ left: m.x, top: m.y }}>
          {sectionRow ? (
            <>
              <button type="button" role="menuitem" onClick={() => act(sectionRow.active ? "deactivate" : "activate")}>
                {sectionRow.active ? "Deactivate Cut" : "Activate Cut"}
              </button>
              <button type="button" role="menuitem" onClick={() => act("reverse")} title="Keep the other side">
                Reverse
              </button>
              <button type="button" role="menuitem" onClick={() => act("slice")} title="The cut's outline as faces in a new group">
                Create Group from Slice
              </button>
              <button type="button" role="menuitem" onClick={() => act("export-section-svg")}>
                Export Slice (SVG)…
              </button>
              <button type="button" role="menuitem" onClick={() => act("export-section-dxf")}>
                Export Slice (DXF)…
              </button>
              <hr />
            </>
          ) : null}
          {nodesOnly && m.rows.length === 1 ? (
            <button type="button" role="menuitem" disabled={m.rows[0].locked} onClick={() => act("edit")}>
              Edit inside
            </button>
          ) : null}
          <button type="button" role="menuitem" disabled={!sameParent || !solidsOrNodes || anyLocked} onClick={() => act("group")}>
            Make Group <kbd>Ctrl+G</kbd>
          </button>
          <button type="button" role="menuitem" disabled={!sameParent || !solidsOrNodes || anyLocked} onClick={() => act("component")}>
            Make Component…
          </button>
          <button type="button" role="menuitem" disabled={!(m.rows.length === 1 && m.rows[0].kind === "instance" && m.rows[0].instances > 1)} onClick={() => act("unique")}>
            Make Unique
          </button>
          <button type="button" role="menuitem" disabled={!nodesOnly || anyLocked} onClick={() => act("explode")}>
            Explode <kbd>Ctrl+Shift+G</kbd>
          </button>
          <hr />
          <button type="button" role="menuitem" disabled={!solidsOrNodes && !anySection} onClick={() => act(anyHidden ? "show" : "hide")}>
            {anyHidden ? "Show" : "Hide"}
          </button>
          <button type="button" role="menuitem" disabled={!solidsOrNodes} onClick={() => act(anyLocked ? "unlock" : "lock")}>
            {anyLocked ? "Unlock" : "Lock"}
          </button>
          <button
            type="button"
            role="menuitem"
            disabled={m.rows.length !== 1 || m.rows[0].kind === "mesh"}
            onClick={() => {
              setMenu(null);
              setRenaming(m.rows[0].key);
            }}
          >
            Rename
          </button>
          <hr />
          <button type="button" role="menuitem" disabled={anyLocked} onClick={() => act("delete")}>
            Delete <kbd>Delete</kbd>
          </button>
        </div>
      ) : null}
    </div>
  );
}
