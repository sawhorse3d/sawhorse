import { useEffect, useState } from "react";
import { CutListPane, type CutListPaneProps } from "./CutListPane";
import type { CreditsHandle } from "../credits/useCredits";
import type { RenderPasses } from "../render/passes";
import { MaterialsPane, type MaterialsPaneProps } from "./MaterialsPane";
import { Outliner, type OutlinerProps } from "./Outliner";
import { PluginsPane, type PluginsPaneProps } from "./PluginsPane";
import { PLUGIN_GLYPH } from "./glyphs";
import { RenderPane } from "./RenderPane";
import { StylesPane, type StylesPaneProps } from "./StylesPane";
import { TagsPane, type TagsPaneProps } from "./TagsPane";

// Right panel: Outliner / Tags / Cut list / Render, as the approved mock. The Outliner and
// Tags panes (lane 5), the Cut list (lane 7) and the Render pane (lane 8) are all live;
// Materials and Styles (with the sun) came in lane 10.

export type PaneId = "outliner" | "tags" | "cutlist" | "materials" | "styles" | "render" | "plugins";

// Plugins (lane 11) is a glyph tab at the end, so the named tabs keep the mock's widths.
const TABS: { id: PaneId; label: string; glyph?: boolean }[] = [
  { id: "outliner", label: "Outliner" },
  { id: "tags", label: "Tags" },
  { id: "cutlist", label: "Cut list" },
  { id: "materials", label: "Materials" },
  { id: "styles", label: "Styles" },
  { id: "render", label: "Render" },
  { id: "plugins", label: "Plugins", glyph: true },
];

export interface SidePanelProps {
  credits: CreditsHandle;
  /** Lane 15: the Render pane is offered only when a render service is set and answering. */
  renderOn?: boolean;
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
  plugins: PluginsPaneProps;
  /** Bumped to bring the Plugins pane forward (the Plugins menu's Manage, an install). */
  focusPlugins?: number;
  /** The pane shown changed (the viewport draws grain arrows while the Cut list is open). */
  onPane?: (pane: PaneId) => void;
}

export function SidePanel({ credits, renderOn = true, capture, focusRender = 0, outliner, tags, focusTags = 0, cutlist, materials, styles, focusMaterials = 0, plugins, focusPlugins = 0, onPane }: SidePanelProps) {
  const [pane, setPane] = useState<PaneId>("outliner");
  useEffect(() => {
    onPane?.(pane);
  }, [pane, onPane]);
  useEffect(() => {
    if (focusRender > 0 && renderOn) setPane("render");
  }, [focusRender, renderOn]);
  useEffect(() => {
    if (!renderOn && pane === "render") setPane("outliner");
  }, [renderOn, pane]);
  useEffect(() => {
    if (focusTags > 0) setPane("tags");
  }, [focusTags]);
  useEffect(() => {
    if (focusMaterials > 0) setPane("materials");
  }, [focusMaterials]);
  useEffect(() => {
    if (focusPlugins > 0) setPane("plugins");
  }, [focusPlugins]);

  return (
    <aside className="side" aria-label="Panels">
      <div className="tabs" role="tablist">
        {TABS.filter((t) => renderOn || t.id !== "render").map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            id={`tab-${t.id}`}
            aria-selected={pane === t.id}
            aria-controls={`pane-${t.id}`}
            data-pane={t.id}
            className={t.glyph ? "glyph-tab" : undefined}
            aria-label={t.glyph ? t.label : undefined}
            title={t.glyph ? t.label : undefined}
            onClick={() => setPane(t.id)}
          >
            {t.glyph ? (
              <svg viewBox="0 0 24 24" aria-hidden="true">
                {PLUGIN_GLYPH}
              </svg>
            ) : (
              t.label
            )}
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

      {renderOn ? (
        <section className="pane" id="pane-render" role="tabpanel" aria-labelledby="tab-render" hidden={pane !== "render"}>
          <RenderPane credits={credits} capture={capture} />
        </section>
      ) : null}

      <section className="pane" id="pane-plugins" role="tabpanel" aria-labelledby="tab-plugins" hidden={pane !== "plugins"}>
        <h3>
          Plugins <small>sandboxed, with permissions</small>
        </h3>
        <PluginsPane {...plugins} />
      </section>
    </aside>
  );
}
