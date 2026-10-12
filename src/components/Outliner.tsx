// The Outliner (lane 5): the model's tree of groups, component instances and solids, laid
// out like the approved mock's Outliner (indented rows, a muted kind hint on the right of
// each label). Each row has an eye (hide / show) and a lock; a click selects (Shift adds,
// Ctrl toggles) and the viewport's selection shows here; a double-click on a name renames
// it; right-click offers Make Group, Make Component, Make Unique, Explode, Hide, Lock,
// Rename and Delete. The open group or component is marked "editing".
import { useEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import type { DocState, Entity } from "../kernel/types";
import { entityKey } from "../lib/entities";
import { chain, graphOf } from "../lib/scope";
import { PanelGlyph } from "./glyphs";

export type OutlinerAction = "edit" | "group" | "component" | "unique" | "explode" | "hide" | "show" | "lock" | "unlock" | "delete";

export interface OutlineRow {
  key: string;
  /** What selecting the row selects (imported meshes are listed, not selectable). */
  entity: Entity | null;
  kind: "group" | "instance" | "solid" | "mesh";
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
}

/** The tree as rows, depth first: in each container its groups and instances (by id), then
 * its solids (in creation order), then its imported meshes. `expanded`: node ids whose
 * contents are listed. */
export function outlineRows(state: DocState | null, expanded: Set<number>): OutlineRow[] {
  const g = graphOf(state);
  const rows: OutlineRow[] = [];
  const names = new Map((state?.object_info ?? []).map((o) => [o.id, o.name]));
  const numbers = new Map((state?.solids ?? []).map((s) => [s.id, s.number]));
  const meshNames = new Map((state?.meshes ?? []).map((m) => [m.id, m.name]));
  const path = new Set(g.path);
  const counter = new Map<number, number>();
  const walk = (parent: number | null, level: number) => {
    for (const n of g.nodes.filter((x) => x.parent === parent)) {
      const def = n.definition !== null ? g.definitions.find((d) => d.id === n.definition) : undefined;
      let hint = "group";
      if (n.kind === "instance") {
        const k = (counter.get(n.definition ?? 0) ?? 0) + 1;
        counter.set(n.definition ?? 0, k);
        hint = def && def.instances > 1 ? `component · ${k} of ${def.instances}` : "component";
      }
      if (n.hidden) hint += " · hidden";
      if (n.tag) hint += ` · ${n.tag}`;
      const children = g.nodes.some((x) => x.parent === n.id) || g.objects.some((o) => o.parent === n.id) || g.meshes.some((m) => m.parent === n.id);
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
        children,
        open: path.has(n.id),
        definition: n.definition,
        instances: def?.instances ?? 0,
      });
      if (expanded.has(n.id)) walk(n.id, level + 1);
    }
    for (const o of g.objects.filter((x) => x.parent === parent)) {
      const tag = state?.object_info.find((x) => x.id === o.id)?.tag;
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
    for (const m of g.meshes.filter((x) => x.parent === parent)) {
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
  };
  walk(null, 0);
  return rows;
}

export interface OutlinerProps {
  state: DocState | null;
  selection: Entity[];
  /** A row was clicked (mode from Shift / Ctrl). */
  onSelect: (row: OutlineRow, mode: "replace" | "add" | "toggle") => void;
  /** A context-menu, eye or lock action on these rows. */
  onAction: (action: OutlinerAction, rows: OutlineRow[]) => void;
  onRename: (row: OutlineRow, name: string | null) => void;
}

interface Menu {
  x: number;
  y: number;
  rows: OutlineRow[];
}

export function Outliner({ state, selection, onSelect, onAction, onRename }: OutlinerProps) {
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
  const selected = useMemo(() => new Set(selection.map(entityKey)), [selection]);
  const isSelected = (r: OutlineRow) => !!r.entity && selected.has(entityKey(r.entity));
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
  const sameParent = m ? new Set(m.rows.map((r) => r.parent)).size === 1 : false;
  const solidsOrNodes = m ? m.rows.every((r) => r.kind !== "mesh") : false;
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
        <ul className="tree outline" role="tree" aria-label="Groups, components and solids">
          {rows.map((r) => {
            const isSel = isSelected(r);
            return (
              <li
                key={r.key}
                role="treeitem"
                aria-selected={isSel}
                aria-expanded={r.children ? expanded.has(nodeOf(r)) : undefined}
                data-row={r.key}
                data-kind={r.kind}
                className={`${isSel ? "sel" : ""}${r.visible ? "" : " dim"}${r.open ? " open" : ""}`}
                style={{ paddingLeft: 4 + r.level * 14 }}
                onClick={(e) => r.entity && onSelect(r, e.ctrlKey || e.metaKey ? "toggle" : e.shiftKey ? "add" : "replace")}
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
                    disabled={r.kind === "mesh"}
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
      {m ? (
        <div className="ctxmenu" ref={menuRef} role="menu" aria-label="Outliner actions" style={{ left: m.x, top: m.y }}>
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
          <button type="button" role="menuitem" disabled={!solidsOrNodes} onClick={() => act(anyHidden ? "show" : "hide")}>
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
