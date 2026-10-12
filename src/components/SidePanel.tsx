import { useEffect, useState } from "react";
import { CutListPane, type CutListPaneProps } from "./CutListPane";
import type { CreditsHandle } from "../credits/useCredits";
import type { RenderPasses } from "../render/passes";
import { MaterialsPane, type MaterialsPaneProps } from "./MaterialsPane";
import { Outliner, type OutlinerProps } from "./Outliner";
import { RenderPane } from "./RenderPane";
import { StylesPane, type StylesPaneProps } from "./StylesPane";
import { TagsPane, type TagsPaneProps } from "./TagsPane";

// Right panel: Outliner / Tags / Cut list / Render, as the approved mock. The Outliner and
// Tags panes (lane 5), the Cut list (lane 7) and the Render pane (lane 8) are all live;
// Materials and Styles (with the sun) came in lane 10.

export type PaneId = "outliner" | "tags" | "cutlist" | "materials" | "styles" | "render";

const TABS: { id: PaneId; label: string }[] = [
  { id: "outliner", label: "Outliner" },
  { id: "tags", label: "Tags" },
  { id: "cutlist", label: "Cut list" },
  { id: "materials", label: "Materials" },
  { id: "styles", label: "Styles" },
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
  materials: MaterialsPaneProps;
  styles: StylesPaneProps;
  /** Bumped to bring the Materials pane forward (the Paint tool chosen with no material). */
  focusMaterials?: number;
  /** The pane shown changed (the viewport draws grain arrows while the Cut list is open). */
  onPane?: (pane: PaneId) => void;
}

export function SidePanel({ credits, capture, focusRender = 0, outliner, tags, focusTags = 0, cutlist, materials, styles, focusMaterials = 0, onPane }: SidePanelProps) {
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
  useEffect(() => {
    if (focusMaterials > 0) setPane("materials");
  }, [focusMaterials]);

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

      <section className="pane" id="pane-materials" role="tabpanel" aria-labelledby="tab-materials" hidden={pane !== "materials"}>
        <h3>
          Materials <small>paint with B</small>
        </h3>
        <MaterialsPane {...materials} />
      </section>

      <section className="pane" id="pane-styles" role="tabpanel" aria-labelledby="tab-styles" hidden={pane !== "styles"}>
        <h3>
          Styles <small>and shadows</small>
        </h3>
        <StylesPane {...styles} />
      </section>

      <section className="pane" id="pane-render" role="tabpanel" aria-labelledby="tab-render" hidden={pane !== "render"}>
        <RenderPane credits={credits} capture={capture} />
      </section>
    </aside>
  );
}
