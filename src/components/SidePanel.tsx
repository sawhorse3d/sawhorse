import { useCallback, useEffect, useRef, useState, type CSSProperties, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";
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

/** Lane 20b: the panel's usual width (px), 360 (the mock's 300 left the Cut list's Part
 * column about 35 px); wider or narrower by dragging its left edge (or the arrow keys on it),
 * wider with the Cut list's Wide button; kept on this device. */
export const SIDE_WIDTH = 360;
/** The narrowest the panel goes (px): the mock's width. */
export const SIDE_MIN = 300;
/** The Wide button's width (px), when the window has room for it. */
export const SIDE_WIDE = 600;
/** Where the width is kept (this browser or this desktop app only). */
export const SIDE_WIDTH_KEY = "sawhorse.sideWidth";
/** The model keeps at least this much room beside the rail (px). */
const VIEW_ROOM = 420;
const RAIL = 56;

/** The widest the panel may be in a window this wide: room is left for the model. */
export function maxSideWidth(windowWidth: number): number {
  return Math.max(SIDE_MIN, Math.min(1000, windowWidth - RAIL - VIEW_ROOM));
}

/** A width clamped to what the window allows, whole pixels. */
export function clampSideWidth(w: number, windowWidth: number): number {
  return Math.round(Math.min(maxSideWidth(windowWidth), Math.max(SIDE_MIN, Number.isFinite(w) ? w : SIDE_WIDTH)));
}

function storedSideWidth(): number {
  try {
    const v = Number(localStorage.getItem(SIDE_WIDTH_KEY));
    return v > 0 ? v : SIDE_WIDTH;
  } catch {
    return SIDE_WIDTH;
  }
}

function saveSideWidth(w: number): void {
  try {
    if (w === SIDE_WIDTH) localStorage.removeItem(SIDE_WIDTH_KEY);
    else localStorage.setItem(SIDE_WIDTH_KEY, String(w));
  } catch {
    /* storage blocked: the width lasts for this session */
  }
}

const windowWidth = () => (typeof window === "undefined" ? 1440 : window.innerWidth);

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
  // Lane 20: the panel's width, dragged on its left edge or set by the Cut list's Wide.
  const [width, setWidthState] = useState(() => clampSideWidth(storedSideWidth(), windowWidth()));
  const setWidth = useCallback((w: number) => {
    const next = clampSideWidth(w, windowWidth());
    setWidthState(next);
    saveSideWidth(next);
  }, []);
  useEffect(() => {
    const fit = () => setWidthState((w) => clampSideWidth(w, windowWidth()));
    window.addEventListener("resize", fit);
    return () => window.removeEventListener("resize", fit);
  }, []);
  const drag = useRef<{ x: number; w: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  const gripDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture?.(e.pointerId);
    drag.current = { x: e.clientX, w: width };
    setDragging(true);
  };
  const gripMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (d) setWidthState(clampSideWidth(d.w + (d.x - e.clientX), windowWidth()));
  };
  const gripUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    drag.current = null;
    setDragging(false);
    setWidth(d.w + (d.x - e.clientX));
  };
  const gripKey = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    const step = e.shiftKey ? 100 : 20;
    if (e.key === "ArrowLeft") setWidth(width + step);
    else if (e.key === "ArrowRight") setWidth(width - step);
    else if (e.key === "Home") setWidth(SIDE_MIN);
    else if (e.key === "End") setWidth(maxSideWidth(windowWidth()));
    else return;
    e.preventDefault();
    e.stopPropagation();
  };
  const wideWidth = clampSideWidth(SIDE_WIDE, windowWidth());
  const wide = width > SIDE_WIDTH;
  /** The select the pointer last went down on (lane 15). */
  const pointerSelect = useRef<HTMLSelectElement | null>(null);
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
    <aside
      className="side"
      aria-label="Panels"
      style={{ "--side-w": `${width}px` } as CSSProperties}
      // Lane 15: a select picked with the mouse gives the keyboard back to the model, so the
      // next "r" or "p" picks a tool instead of jumping the select to an option. (A select
      // worked from the keyboard keeps focus: its arrows still move through the options.)
      onPointerDownCapture={(e) => {
        pointerSelect.current = e.target instanceof HTMLSelectElement ? e.target : null;
      }}
      onKeyDownCapture={() => {
        pointerSelect.current = null;
      }}
      onChangeCapture={(e) => {
        const t = e.target;
        if (t instanceof HTMLSelectElement && t === pointerSelect.current) {
          pointerSelect.current = null;
          // After React has handled the change.
          setTimeout(() => {
            if (document.activeElement === t) t.blur();
          }, 0);
        }
      }}
    >
      <div
        className={`side-grip${dragging ? " on" : ""}`}
        role="separator"
        aria-orientation="vertical"
        aria-label="Panel width"
        aria-valuemin={SIDE_MIN}
        aria-valuemax={maxSideWidth(windowWidth())}
        aria-valuenow={width}
        tabIndex={0}
        title="Drag to widen or narrow the panel (arrow keys too; double-click for the usual width)"
        data-testid="side-grip"
        onPointerDown={gripDown}
        onPointerMove={gripMove}
        onPointerUp={gripUp}
        onPointerCancel={gripUp}
        onDoubleClick={() => setWidth(SIDE_WIDTH)}
        onKeyDown={gripKey}
      />
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
        <CutListPane {...cutlist} wide={wide} panelWidth={width} onWide={wideWidth > SIDE_WIDTH ? (w) => setWidth(w ? wideWidth : SIDE_WIDTH) : undefined} />
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
