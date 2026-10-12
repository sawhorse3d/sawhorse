// The Tags pane (lane 5), laid out like the approved mock's: a colour swatch, the name, a
// muted note. "Untagged" (the default) comes first. Each tag can be shown or hidden (the
// eye), recoloured (the swatch), renamed (double-click the name) and deleted; with
// something selected, "Tag" puts it on that tag. Tags control visibility only: nothing
// moves, and geometry stays in its group or component.
import { useMemo, useState } from "react";
import type { DocState } from "../kernel/types";
import { graphOf } from "../lib/scope";
import { PanelGlyph } from "./glyphs";

/** The swatch colour of "Untagged" and of tags without one. */
export const UNTAGGED_COLOR = "#3a3f3b";

/** Colours handed to new tags in turn (our palette, picked to read on light and dark). */
export const TAG_PALETTE = ["#c48a2a", "#6a9b5a", "#8a5cd6", "#3b8fd9", "#c2417f", "#17a2a0", "#9aa39a", "#d9663a"];

export function hexToRgb(hex: string): [number, number, number] {
  const v = parseInt(hex.replace("#", ""), 16);
  return [(v >> 16) & 255, (v >> 8) & 255, v & 255];
}

/** How many solids, groups and components are on each tag (and on none). What is inside a
 * component is counted once, in its definition, not once per copy. */
export function tagCounts(state: DocState | null): Map<string | null, number> {
  const g = graphOf(state);
  const out = new Map<string | null, number>();
  const bump = (t: string | null) => out.set(t, (out.get(t) ?? 0) + 1);
  const inside = new Set(g.objects.filter((o) => o.in_component).map((o) => o.id));
  for (const o of state?.object_info ?? []) if (!inside.has(o.id)) bump(o.tag);
  for (const n of g.nodes) if (!n.in_component) bump(n.tag);
  return out;
}

export interface TagsPaneProps {
  state: DocState | null;
  /** Something is selected that a tag can go on. */
  canTag: boolean;
  onCreate: () => void;
  onVisible: (tag: string, visible: boolean) => void;
  onColor: (tag: string, color: string) => void;
  onRename: (tag: string, name: string) => void;
  onDelete: (tag: string) => void;
  /** Put the selection on `tag` (null: Untagged). */
  onAssign: (tag: string | null) => void;
  /** The tag just created: its name starts in edit mode. */
  editing?: string | null;
  onEdited?: () => void;
}

export function TagsPane({ state, canTag, onCreate, onVisible, onColor, onRename, onDelete, onAssign, editing = null, onEdited }: TagsPaneProps) {
  const g = graphOf(state);
  const tags = useMemo(() => Object.entries(state?.tags ?? {}).sort(([a], [b]) => a.localeCompare(b)), [state]);
  const counts = useMemo(() => tagCounts(state), [state]);
  const [renaming, setRenaming] = useState<string | null>(null);
  const live = renaming ?? editing;
  const plural = (n: number) => `${n} item${n === 1 ? "" : "s"}`;

  return (
    <div className="tags-pane" data-testid="tags">
      <ul className="tree tags">
        <li data-tag="">
          <span className="tag" style={{ background: UNTAGGED_COLOR }} />
          <span className="label">Untagged</span>
          <span className="k">{`default · ${plural(counts.get(null) ?? 0)}`}</span>
          <span className="row-tools">
            <button type="button" disabled={!canTag} title="Take the selection off its tag" aria-label="Put the selection on Untagged" onClick={() => onAssign(null)}>
              <PanelGlyph name="tag" />
            </button>
          </span>
        </li>
        {tags.map(([name, visible]) => {
          const color = g.tag_colors[name] ?? UNTAGGED_COLOR;
          return (
            <li key={name} data-tag={name} className={visible ? "" : "dim"}>
              <label className="swatch" title="Colour">
                <span className="tag" style={{ background: color }} />
                <input type="color" aria-label={`Colour of ${name}`} value={color} onChange={(e) => onColor(name, e.currentTarget.value)} />
              </label>
              {live === name ? (
                <input
                  className="rename"
                  aria-label={`New name for tag ${name}`}
                  autoFocus
                  defaultValue={name}
                  onFocus={(e) => e.currentTarget.select()}
                  onKeyDown={(e) => {
                    e.stopPropagation();
                    if (e.key === "Enter") {
                      const v = e.currentTarget.value.trim();
                      setRenaming(null);
                      onEdited?.();
                      if (v && v !== name) onRename(name, v);
                    } else if (e.key === "Escape") {
                      setRenaming(null);
                      onEdited?.();
                    }
                  }}
                  onBlur={() => {
                    setRenaming(null);
                    onEdited?.();
                  }}
                />
              ) : (
                <span className="label" title="Double-click to rename" onDoubleClick={() => setRenaming(name)}>
                  {name}
                </span>
              )}
              <span className="k">{`${visible ? "" : "hidden · "}${plural(counts.get(name) ?? 0)}`}</span>
              <span className="row-tools">
                <button type="button" aria-label={visible ? `Hide tag ${name}` : `Show tag ${name}`} aria-pressed={!visible} title={visible ? "Hide" : "Show"} onClick={() => onVisible(name, !visible)}>
                  <PanelGlyph name={visible ? "eye" : "eye-off"} />
                </button>
                <button type="button" disabled={!canTag} aria-label={`Put the selection on ${name}`} title="Put the selection on this tag" onClick={() => onAssign(name)}>
                  <PanelGlyph name="tag" />
                </button>
                <button type="button" aria-label={`Delete tag ${name}`} title="Delete (what is on it becomes untagged)" onClick={() => onDelete(name)}>
                  <PanelGlyph name="trash" />
                </button>
              </span>
            </li>
          );
        })}
      </ul>
      <div className="btnrow">
        <button type="button" className="btn" onClick={() => onCreate()}>
          <PanelGlyph name="plus" /> New tag
        </button>
      </div>
    </div>
  );
}
