import { useEffect, useState } from "react";
import { CutListPane, type CutListPaneProps } from "./CutListPane";
import type { CreditsHandle } from "../credits/useCredits";
import type { RenderPasses } from "../render/passes";
import { Outliner, type OutlinerProps } from "./Outliner";
import { RenderPane } from "./RenderPane";
import { TagsPane, type TagsPaneProps } from "./TagsPane";

// Right panel: Outliner / Tags / Cut list / Render, as the approved mock. The Outliner and
// Tags panes (lane 5), the Cut list (lane 7) and the Render pane (lane 8) are all live.

export type PaneId = "outliner" | "tags" | "cutlist" | "render";

const TABS: { id: PaneId; label: string }[] = [
  { id: "outliner", label: "Outliner" },
  { id: "tags", label: "Tags" },
  { id: "cutlist", label: "Cut list" },
  { id: "render", label: "Render" },
];

export interface SidePanelProps {
  credits: CreditsHandle;
  /** Draw the render passes from the current view (the viewport engine). */
  capture: () => Promise<RenderPasses>;
  /** Bumped to bring the Render pane forward (the Credits chip). */
  focusRender?: number;
  outliner: OutlinerProps;
  tags: TagsPaneProps;
  /** Bumped to bring the Tags pane forward (a tag just created from elsewhere). */
  focusTags?: number;
  cutlist: CutListPaneProps;
  /** The pane shown changed (the viewport draws grain arrows while the Cut list is open). */
  onPane?: (pane: PaneId) => void;
}

export function SidePanel({ credits, capture, focusRender = 0, outliner, tags, focusTags = 0, cutlist, onPane }: SidePanelProps) {
  const [pane, setPane] = useState<PaneId>("outliner");
  useEffect(() => {
    onPane?.(pane);
  }, [pane, onPane]);
  useEffect(() => {
    if (focusRender > 0) setPane("render");
  }, [focusRender]);
  useEffect(() => {
    if (focusTags > 0) setPane("tags");
  }, [focusTags]);

  return (
    <aside className="side" aria-label="Panels">
      <div className="tabs" role="tablist">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            id={`tab-${t.id}`}
            aria-selected={pane === t.id}
            aria-controls={`pane-${t.id}`}
            data-pane={t.id}
            onClick={() => setPane(t.id)}
          >
            {t.label}
          </button>
        ))}
      </div>

      <section className="pane" id="pane-outliner" role="tabpanel" aria-labelledby="tab-outliner" hidden={pane !== "outliner"}>
        <h3>
          Outliner <small>groups and components</small>
        </h3>
        <Outliner {...outliner} />
        <p className="note">Double-click in the model to edit in place (Esc leaves). Double-click a name to rename. Right-click for Make Group, Make Component, Make Unique, Explode, Lock.</p>
      </section>

      <section className="pane" id="pane-tags" role="tabpanel" aria-labelledby="tab-tags" hidden={pane !== "tags"}>
        <h3>Tags</h3>
        <TagsPane {...tags} />
        <p className="note">Tags control visibility only. Geometry stays in its group.</p>
      </section>

      <section className="pane" id="pane-cutlist" role="tabpanel" aria-labelledby="tab-cutlist" hidden={pane !== "cutlist"}>
        <h3>
          Cut list <small>from components</small>
        </h3>
        <CutListPane {...cutlist} />
      </section>

      <section className="pane" id="pane-render" role="tabpanel" aria-labelledby="tab-render" hidden={pane !== "render"}>
        <RenderPane credits={credits} capture={capture} />
      </section>
    </aside>
  );
}
