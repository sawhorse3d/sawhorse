import { useEffect, useState } from "react";
import type { CreditsHandle } from "../credits/useCredits";
import type { RenderPasses } from "../render/passes";
import { RenderPane } from "./RenderPane";

// Right panel: Outliner / Tags / Cut list / Render. The Render pane is live (lane 8: render
// service and credit store); the others still carry the approved mock's example content
// until their lanes land (groups/components, cut list).

export type PaneId = "outliner" | "tags" | "cutlist" | "render";

const TABS: { id: PaneId; label: string }[] = [
  { id: "outliner", label: "Outliner" },
  { id: "tags", label: "Tags" },
  { id: "cutlist", label: "Cut list" },
  { id: "render", label: "Render" },
];

const CUT_LIST = [
  { part: "Side", qty: 2, t: "3/4", w: "11 1/4", l: "72", material: "Oak S4S" },
  { part: "Top", qty: 1, t: "3/4", w: "11 1/4", l: "35 1/4", material: "Oak S4S" },
  { part: "Shelf", qty: 4, t: "3/4", w: "11", l: "34 1/2", material: "Oak S4S" },
  { part: "Back panel", qty: 1, t: "1/4", w: "35 1/4", l: "71 1/4", material: "Oak ply" },
  { part: "Toe kick", qty: 1, t: "3/4", w: "3", l: "34 1/2", material: "Oak S4S" },
];

export interface SidePanelProps {
  credits: CreditsHandle;
  /** Draw the render passes from the current view (the viewport engine). */
  capture: () => Promise<RenderPasses>;
  /** Bumped to bring the Render pane forward (the Credits chip). */
  focusRender?: number;
}

export function SidePanel({ credits, capture, focusRender = 0 }: SidePanelProps) {
  const [pane, setPane] = useState<PaneId>("outliner");
  useEffect(() => {
    if (focusRender > 0) setPane("render");
  }, [focusRender]);

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
        <ul className="tree">
          <li><span className="k">▾</span> Bookshelf <span className="k">group</span></li>
          <li className="lvl1 sel"><span className="k">▸</span> Side L <span className="k">component · 1 of 2</span></li>
          <li className="lvl1"><span className="k">▸</span> Side R <span className="k">component · 2 of 2</span></li>
          <li className="lvl1"><span className="k">▸</span> Shelf <span className="k">component · 4 instances</span></li>
          <li className="lvl1"><span className="k">▸</span> Top</li>
          <li className="lvl1"><span className="k">▸</span> Back panel <span className="k">1/4 ply</span></li>
          <li><span className="k">▸</span> Dado jig <span className="k">group · hidden</span></li>
        </ul>
        <p className="note">Double-click to edit in place. Right-click for Make Unique, Explode, Lock.</p>
      </section>

      <section className="pane" id="pane-tags" role="tabpanel" aria-labelledby="tab-tags" hidden={pane !== "tags"}>
        <h3>Tags</h3>
        <ul className="tree">
          <li><span className="tag" style={{ background: "#3a3f3b" }}></span> Untagged <span className="k">default</span></li>
          <li><span className="tag" style={{ background: "#c48a2a" }}></span> Carcass</li>
          <li><span className="tag" style={{ background: "#6a9b5a" }}></span> Shelves</li>
          <li><span className="tag" style={{ background: "#8a5cd6" }}></span> Hardware</li>
          <li><span className="tag" style={{ background: "#9aa39a" }}></span> Jigs <span className="k">hidden</span></li>
        </ul>
        <p className="note">Tags control visibility only. Geometry stays in its group.</p>
      </section>

      <section className="pane" id="pane-cutlist" role="tabpanel" aria-labelledby="tab-cutlist" hidden={pane !== "cutlist"}>
        <h3>
          Cut list <small>from components</small>
        </h3>
        <div className="tablewrap">
          <table>
            <thead>
              <tr>
                <th>Part</th>
                <th className="n">Qty</th>
                <th className="n">T</th>
                <th className="n">W</th>
                <th className="n">L</th>
                <th>Material</th>
              </tr>
            </thead>
            <tbody>
              {CUT_LIST.map((r) => (
                <tr key={r.part}>
                  <td>{r.part}</td>
                  <td className="n">{r.qty}</td>
                  <td className="n">{r.t}</td>
                  <td className="n">{r.w}</td>
                  <td className="n">{r.l}</td>
                  <td>{r.material}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="sum">
          <div>
            <b>27.6</b>
            <span>board feet, solid oak</span>
          </div>
          <div>
            <b>1</b>
            <span>sheet 4×8 ply, 1/4</span>
          </div>
        </div>
        <div className="btnrow">
          <button type="button" className="btn">CSV</button>
          <button type="button" className="btn">PDF</button>
          <button type="button" className="btn">Labels</button>
          <button type="button" className="btn" title="Week 4" disabled>
            Nesting ›
          </button>
        </div>
        <p className="note">Grain runs along L. Dimensions are finished sizes, kerf not included until nesting.</p>
      </section>

      <section className="pane" id="pane-render" role="tabpanel" aria-labelledby="tab-render" hidden={pane !== "render"}>
        <RenderPane credits={credits} capture={capture} />
      </section>
    </aside>
  );
}
