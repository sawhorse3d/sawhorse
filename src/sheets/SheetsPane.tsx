// The Sheets pane (lane 12): the side panel in Sheets mode. The sheets in order (add,
// rename, reorder, delete), the options of the tool in hand, the selected item's
// properties, the paper (size, orientation, margins, units, grid, how dimensions write
// lengths), the title block, and printing and exporting. Text fields change the sheet when
// they are left or Enter is pressed, so one edit is one undo step, not one per letter.
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { CutColumn, DimKind, DimStyle, ItemKind, LengthFormat, PaperSizeKind, Scale, SheetRow, SheetSetup, TitleBlock, ViewportSource } from "../kernel/sheetTypes";
import type { SceneDef } from "../kernel/types";
import { CUT_COLUMNS, FORMATS, ITEM_NAMES, PAPERS, STANDARD_SCALES, STD_VIEWS, canBeToScale, captionFor, formatKey, localDate, paperSize, paperText, parsePaper, scaleKey, scaleLabel, sourceLabel } from "./paper";
import type { SheetRender } from "./render";
import type { SheetToolId } from "./SheetCanvas";

import type { SheetExportKind } from "./runExport";
export type { SheetExportKind };

const EXPORTS: { kind: SheetExportKind; label: string; title: string; primary?: boolean }[] = [
  { kind: "print", label: "Print…", title: "This sheet through the system print dialog, at 100% (it can also save a PDF)", primary: true },
  { kind: "pdf", label: "PDF", title: "This sheet as a vector PDF at its paper size" },
  { kind: "pdf-all", label: "PDF, all sheets", title: "Every sheet in order, a page each" },
  { kind: "svg", label: "SVG", title: "This sheet as SVG, in paper units" },
  { kind: "dxf", label: "DXF", title: "This sheet as DXF at paper size: lines, outlines and text" },
  { kind: "png150", label: "PNG 150 dpi", title: "This sheet as an image" },
  { kind: "png300", label: "PNG 300 dpi", title: "This sheet as an image, print quality" },
];

export interface SheetsPaneProps {
  sheets: SheetRow[];
  active: number | null;
  render: SheetRender | null;
  selection: number[];
  scenes: SceneDef[];
  tool: SheetToolId;
  /** The source a new viewport shows. */
  newSource: ViewportSource;
  onNewSource: (s: ViewportSource) => void;
  dimKind: DimKind;
  onDimKind: (k: DimKind) => void;
  /** Resolves to something truthy when the kernel took the edit (null or false: refused). */
  onSetup: (id: number, setup: SheetSetup) => unknown;
  /** Resolves to something truthy when the kernel took the edit (null or false: refused). */
  onSetItem: (id: number, kind: ItemKind) => unknown;
  onDeleteItems: (ids: number[]) => void;
  onOrder: (id: number, to: number) => void;
  /** Set a viewport to the largest standard scale its drawing fits at. */
  onFitScale: (id: number) => void;
  /** Point a viewport at the 3D view's camera. */
  onCurrentView: (id: number) => void;
  onExport: (kind: SheetExportKind) => void;
  /** Which exports exist yet. */
  exports: SheetExportKind[];
}

/** A sheet's setup: everything but its items. */
export function setupOf(row: SheetRow): SheetSetup {
  const { items: _items, next_item: _next, ...setup } = row.sheet;
  void _items;
  void _next;
  return setup;
}

/** A text field that commits on Enter or when left (Esc puts it back). */
export function Field({ label, value, onCommit, placeholder, multiline, mono, testId }: { label: string; value: string; onCommit: (v: string) => void; placeholder?: string; multiline?: boolean; mono?: boolean; testId?: string }) {
  const [v, setV] = useState(value);
  useEffect(() => setV(value), [value]);
  const commit = () => {
    if (v !== value) onCommit(v);
  };
  const common = {
    "aria-label": label,
    "data-testid": testId,
    value: v,
    placeholder,
    className: mono ? "mono" : undefined,
    onChange: (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => setV(e.currentTarget.value),
    onBlur: commit,
    onKeyDown: (e: React.KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>) => {
      e.stopPropagation();
      if (e.key === "Enter" && !(multiline && e.shiftKey)) {
        e.preventDefault();
        commit();
        (e.currentTarget as HTMLElement).blur();
      } else if (e.key === "Escape") {
        setV(value);
        (e.currentTarget as HTMLElement).blur();
      }
    },
  };
  return (
    <label className="sp-field">
      <span>{label}</span>
      {multiline ? <textarea rows={3} {...common} /> : <input type="text" {...common} />}
    </label>
  );
}

/** A paper length field in the sheet's units. */
function LengthField({ label, value, units, onCommit, min = 0, testId }: { label: string; value: number; units: SheetSetup["units"]; onCommit: (m: number) => void; min?: number; testId?: string }) {
  const [bad, setBad] = useState(false);
  return (
    <div className={bad ? "sp-bad" : undefined}>
      <Field
        label={label}
        testId={testId}
        mono
        value={paperText(value, units ?? "inches")}
        onCommit={(t) => {
          const m = parsePaper(t, units ?? "inches");
          setBad(m === null || m < min);
          if (m !== null && m >= min) onCommit(m);
        }}
      />
    </div>
  );
}

function Section({ title, children, open = true, testId }: { title: string; children: ReactNode; open?: boolean; testId?: string }) {
  return (
    <details className="sp-section" open={open} data-testid={testId}>
      <summary>{title}</summary>
      <div className="sp-body">{children}</div>
    </details>
  );
}

function sourceKey(s: ViewportSource): string {
  return s.from === "scene" ? `scene:${s.name}` : s.from === "standard" ? `std:${s.view}` : "camera";
}

function SourceSelect({ value, scenes, onChange, allowCamera, label }: { value: ViewportSource; scenes: SceneDef[]; onChange: (s: ViewportSource) => void; allowCamera: boolean; label: string }) {
  return (
    <label className="sp-field">
      <span>{label}</span>
      <select
        aria-label={label}
        value={sourceKey(value)}
        onChange={(e) => {
          const v = e.currentTarget.value;
          if (v.startsWith("scene:")) onChange({ from: "scene", name: v.slice(6) });
          else if (v.startsWith("std:")) onChange({ from: "standard", view: v.slice(4) as never });
        }}
      >
        {scenes.length ? (
          <optgroup label="Scenes">
            {scenes.map((s) => (
              <option key={s.name} value={`scene:${s.name}`}>
                {s.name}
                {s.parallel ? "" : " (perspective)"}
              </option>
            ))}
          </optgroup>
        ) : null}
        <optgroup label="Standard views">
          {STD_VIEWS.map((v) => (
            <option key={v.view} value={`std:${v.view}`}>
              {v.label}
            </option>
          ))}
        </optgroup>
        {allowCamera && value.from === "camera" ? <option value="camera">Saved camera</option> : null}
      </select>
    </label>
  );
}

function ScaleSelect({ value, toScale, onChange }: { value: Scale; toScale: boolean; onChange: (s: Scale) => void }) {
  const known = STANDARD_SCALES.some((s) => scaleKey(s) === scaleKey(value)) || value.kind === "nts";
  return (
    <label className="sp-field">
      <span>Scale</span>
      <select
        aria-label="Scale"
        value={scaleKey(value)}
        onChange={(e) => {
          const k = e.currentTarget.value;
          if (k === "NTS") onChange({ kind: "nts" });
          else {
            const s = STANDARD_SCALES.find((x) => scaleKey(x) === k);
            if (s) onChange(s);
          }
        }}
      >
        <option value="NTS">Not to scale (fit)</option>
        {toScale ? (
          <>
            <optgroup label="Ratio">
              {STANDARD_SCALES.filter((s) => s.kind === "ratio").map((s) => (
                <option key={scaleKey(s)} value={scaleKey(s)}>
                  {scaleLabel(s)}
                </option>
              ))}
            </optgroup>
            <optgroup label="Architectural">
              {STANDARD_SCALES.filter((s) => s.kind === "architect").map((s) => (
                <option key={scaleKey(s)} value={scaleKey(s)}>
                  {scaleLabel(s)}
                </option>
              ))}
            </optgroup>
          </>
        ) : null}
        {!known ? <option value={scaleKey(value)}>{scaleLabel(value)}</option> : null}
      </select>
    </label>
  );
}

/**
 * Edits on their way to the kernel. Each commit sends a whole value (a sheet's setup, an
 * item), built from what the pane shows; the pane shows the kernel's state, which arrives a
 * moment after the edit. Two fields committed within that moment (Enter in Title, then
 * Project) would each send the old value of the other, and the second would undo the
 * first. So the value last sent is kept, with the state it was built from (`from`, compared
 * by identity): until a new state arrives, the next commit builds on the value sent, not on
 * the props. A refusal changes no state, so it ends the edit at once.
 */
export function useInFlight<K, T>() {
  const sent = useRef<{ from: K; value: T } | null>(null);
  return {
    /** The value to build the next edit on: the one sent, while `from` is still current. */
    base: (from: K, shown: T): T => (sent.current && sent.current.from === from ? sent.current.value : shown),
    send: (from: K, value: T, go: (v: T) => unknown) => {
      const token = { from, value };
      sent.current = token;
      void Promise.resolve(go(value)).then(
        (ok) => {
          if (!ok && sent.current === token) sent.current = null;
        },
        () => {
          if (sent.current === token) sent.current = null;
        },
      );
    },
  };
}

/** The top-level fields `next` changes relative to `shown`, laid over `base`. */
function rebase<T extends object>(base: T, shown: T, next: T): T {
  const out: Record<string, unknown> = { ...(base as Record<string, unknown>) };
  const n = next as Record<string, unknown>;
  const was = shown as Record<string, unknown>;
  for (const key of new Set([...Object.keys(n), ...Object.keys(was)])) if (n[key] !== was[key]) out[key] = n[key];
  return out as T;
}

const DIM_KINDS: { kind: DimKind; label: string }[] = [
  { kind: "horizontal", label: "Horizontal" },
  { kind: "vertical", label: "Vertical" },
  { kind: "aligned", label: "Aligned" },
  { kind: "angular", label: "Angle" },
  { kind: "radius", label: "Radius" },
  { kind: "diameter", label: "Diameter" },
];

function ItemProps({ id, kind, props, render }: { id: number; kind: ItemKind; props: SheetsPaneProps; render: SheetRender | null }) {
  // `kind` is what the props hold; a commit lays the fields it changes over any edit of this
  // item still on its way (see useInFlight).
  const flight = useInFlight<ItemKind, ItemKind>();
  const set = (k: ItemKind) => flight.send(kind, rebase(flight.base(kind, kind), kind, k), (v) => props.onSetItem(id, v));
  const sheet = props.sheets.find((s) => s.id === props.active)?.sheet;
  const units = sheet?.units ?? "inches";
  const count = sheet?.items.length ?? 0;
  const index = sheet?.items.findIndex((i) => i.id === id) ?? -1;
  const info = render?.viewports.find((v) => v.id === id);
  const body = (() => {
    switch (kind.kind) {
      case "viewport": {
        const toScale = canBeToScale(kind.source, props.scenes);
        // A caption Sawhorse wrote ("FRONT ELEVATION 1:12") follows the view and the scale;
        // one typed by hand stays.
        const auto = !kind.caption || kind.caption === captionFor(kind.source, kind.scale);
        const caption = (source: ViewportSource, scale: Scale) => (auto ? captionFor(source, scale) : kind.caption);
        return (
          <>
            <SourceSelect
              label="Shows"
              value={kind.source}
              scenes={props.scenes}
              allowCamera
              onChange={(source) => {
                const scale: Scale = canBeToScale(source, props.scenes) ? kind.scale : { kind: "nts" };
                set({ ...kind, source, centre: null, scale, caption: caption(source, scale) });
              }}
            />
            <ScaleSelect value={kind.scale} toScale={toScale} onChange={(scale) => set({ ...kind, scale, caption: caption(kind.source, scale) })} />
            {!toScale ? <p className="note">An isometric or perspective view is never to scale: it fits its frame.</p> : null}
            {info?.problem ? <p className="note err">Cannot draw: {info.problem}</p> : null}
            <Field label="Caption" value={kind.caption ?? ""} placeholder="No caption" onCommit={(c) => set({ ...kind, caption: c.trim() ? c : null })} />
            <label className="sp-check">
              <input type="checkbox" checked={!!kind.hidden_lines} onChange={(e) => set({ ...kind, hidden_lines: e.currentTarget.checked })} /> Hidden lines dashed
            </label>
            <label className="sp-check">
              <input type="checkbox" checked={!!kind.border} onChange={(e) => set({ ...kind, border: e.currentTarget.checked })} /> Frame
            </label>
            <div className="btnrow">
              <button type="button" className="btn" disabled={!toScale} onClick={() => props.onFitScale(id)} title="The largest standard scale the drawing fits at">
                Fit scale
              </button>
              <button type="button" className="btn" disabled={kind.centre == null} onClick={() => set({ ...kind, centre: null })} title="Centre the drawing in the frame (Alt+drag slides it)">
                Recentre
              </button>
              <button type="button" className="btn" onClick={() => props.onCurrentView(id)} title="Show what the 3D view shows now (its camera is kept with the viewport)">
                Use 3D view
              </button>
            </div>
          </>
        );
      }
      case "dimension":
        return (
          <>
            <p className="sp-read">
              {DIM_KINDS.find((d) => d.kind === kind.dim)?.label} dimension: <b>{render?.items.find((i) => i.id === id)?.text ?? "?"}</b>
            </p>
            <Field label="Text" value={kind.text ?? ""} placeholder="The measured length" onCommit={(t) => set({ ...kind, text: t.trim() ? t : null })} />
            <label className="sp-field">
              <span>Ends</span>
              <select aria-label="Ends" value={kind.style ?? "arrows"} onChange={(e) => set({ ...kind, style: e.currentTarget.value as DimStyle })}>
                <option value="arrows">Arrows</option>
                <option value="ticks">Ticks</option>
                <option value="dots">Dots</option>
              </select>
            </label>
            <LengthField label="Stand-off" value={Math.abs(kind.offset)} units={units} onCommit={(offset) => set({ ...kind, offset: kind.offset < 0 ? -offset : offset })} />
            <p className="note">Drag the dimension to stand it off; it follows its viewport.</p>
          </>
        );
      case "label":
        return (
          <>
            <Field label="Text" multiline value={kind.text} onCommit={(text) => text.trim() && set({ ...kind, text })} />
            <LengthField label="Text height" value={kind.size} units={units} onCommit={(size) => size > 0 && set({ ...kind, size })} />
            {kind.leader ? (
              <button type="button" className="btn" onClick={() => set({ ...kind, leader: null })}>
                Remove leader
              </button>
            ) : null}
          </>
        );
      case "table":
        return (
          <>
            <Field label="Title" value={kind.title ?? ""} placeholder="No title" onCommit={(t) => set({ ...kind, title: t.trim() ? t : null })} />
            <fieldset className="sp-cols">
              <legend>Columns</legend>
              {CUT_COLUMNS.map((c) => (
                <label key={c.id} className="sp-check">
                  <input
                    type="checkbox"
                    checked={kind.columns.includes(c.id)}
                    onChange={(e) => {
                      const on = e.currentTarget.checked;
                      const columns: CutColumn[] = CUT_COLUMNS.map((x) => x.id).filter((x) => (x === c.id ? on : kind.columns.includes(x)));
                      if (columns.length) set({ ...kind, columns });
                    }}
                  />{" "}
                  {c.label}
                </label>
              ))}
            </fieldset>
            <label className="sp-check">
              <input type="checkbox" checked={!!kind.decimal} onChange={(e) => set({ ...kind, decimal: e.currentTarget.checked })} /> Decimal sizes
            </label>
            <label className="sp-check">
              <input type="checkbox" checked={!!kind.visible_only} onChange={(e) => set({ ...kind, visible_only: e.currentTarget.checked })} /> Only what is shown
            </label>
          </>
        );
      case "image":
        return <p className="sp-read">Image {kind.path.replace(/^.*\//, "")}</p>;
      case "line":
        return (
          <>
            <LengthField label="Pen width" value={kind.width} units={units} onCommit={(width) => width > 0 && set({ ...kind, width })} />
            <label className="sp-check">
              <input type="checkbox" checked={!!kind.dashed} onChange={(e) => set({ ...kind, dashed: e.currentTarget.checked })} /> Dashed
            </label>
          </>
        );
      case "rect":
        return (
          <>
            <LengthField label="Pen width" value={kind.width} units={units} onCommit={(width) => set({ ...kind, width })} />
            <label className="sp-check">
              <input type="checkbox" checked={!!kind.fill} onChange={(e) => set({ ...kind, fill: e.currentTarget.checked ? [235, 235, 230] : null })} /> Filled
            </label>
          </>
        );
    }
  })();
  const rect = kind.kind === "viewport" || kind.kind === "table" || kind.kind === "image" || kind.kind === "rect" ? kind.rect : null;
  return (
    <div className="sp-item" data-item-kind={kind.kind}>
      {body}
      {rect ? (
        <div className="sp-grid2">
          <LengthField label="X" value={rect[0]} units={units} min={-2} onCommit={(x) => set({ ...kind, rect: [x, rect[1], rect[2], rect[3]] } as ItemKind)} />
          <LengthField label="Y" value={rect[1]} units={units} min={-2} onCommit={(y) => set({ ...kind, rect: [rect[0], y, rect[2], rect[3]] } as ItemKind)} />
          <LengthField label="Width" value={rect[2]} units={units} onCommit={(w) => w > 0.001 && set({ ...kind, rect: [rect[0], rect[1], w, rect[3]] } as ItemKind)} />
          <LengthField label="Height" value={rect[3]} units={units} onCommit={(h) => h > 0.001 && set({ ...kind, rect: [rect[0], rect[1], rect[2], h] } as ItemKind)} />
        </div>
      ) : null}
      <div className="btnrow">
        <button type="button" className="btn" disabled={index < 0 || index === count - 1} onClick={() => props.onOrder(id, count - 1)}>
          Bring to front
        </button>
        <button type="button" className="btn" disabled={index <= 0} onClick={() => props.onOrder(id, 0)}>
          Send to back
        </button>
        <button type="button" className="btn" onClick={() => props.onDeleteItems([id])}>
          Delete
        </button>
      </div>
    </div>
  );
}

export function SheetsPane(props: SheetsPaneProps) {
  const { sheets, active, render, selection, scenes, tool } = props;
  const row = sheets.find((s) => s.id === active) ?? null;
  const setup = row ? setupOf(row) : null;
  // A change of the setup builds on any change still on its way (see useInFlight).
  const flight = useInFlight<SheetRow, SheetSetup>();
  const put = (next: Partial<SheetSetup> | ((s: SheetSetup) => Partial<SheetSetup>)) => {
    if (!row || !setup) return;
    const base = flight.base(row, setup);
    flight.send(row, { ...base, ...(typeof next === "function" ? next(base) : next) }, (v) => props.onSetup(row.id, v));
  };
  const title = (next: Partial<TitleBlock>) => put((s) => ({ title: { ...(s.title ?? {}), ...next } }));
  const selected = row && selection.length === 1 ? row.sheet.items.find((i) => i.id === selection[0]) : undefined;
  const units = setup?.units ?? "inches";

  return (
    <aside className="side sheet-side" aria-label="Sheet panels">
      <div className="tabs" role="tablist">
        <button type="button" role="tab" aria-selected="true" id="tab-sheet">
          Sheet
        </button>
      </div>
      <section className="pane sheets-pane" role="tabpanel" aria-labelledby="tab-sheet">
        {!row || !setup ? (
          <p className="placeholder">No sheet yet. Add one with the + beside the sheet tabs: a Letter page in landscape with a title block, ready for viewports of the model.</p>
        ) : (
          <>
            {tool === "viewport" ? (
              <Section title="New viewport" testId="tool-viewport">
                <SourceSelect label="Shows" value={props.newSource} scenes={scenes} allowCamera={false} onChange={props.onNewSource} />
                <p className="note">Drag a frame on the paper (or click for a 4 x 3 in one). Square views come in at the largest standard scale that fits.</p>
              </Section>
            ) : null}
            {tool === "dimension" ? (
              <Section title="New dimension" testId="tool-dimension">
                <div className="sp-seg" role="radiogroup" aria-label="Dimension kind">
                  {DIM_KINDS.map((d) => (
                    <button key={d.kind} type="button" role="radio" aria-checked={props.dimKind === d.kind} onClick={() => props.onDimKind(d.kind)}>
                      {d.label}
                    </button>
                  ))}
                </div>
                <p className="note">Click points of a viewport's drawing (two; three for an angle, the corner first, or for an arc), then where the line goes. It measures the model, whatever the scale.</p>
              </Section>
            ) : null}
            {tool === "label" ? (
              <Section title="New label" testId="tool-label">
                <p className="note">Click what it points at (a point of the drawing, or the paper), then where the text goes, then type. Enter places it; Shift+Enter starts a new line.</p>
              </Section>
            ) : null}

            {selected ? (
              <Section title={ITEM_NAMES[selected.kind]} testId="item-props">
                <ItemProps key={selected.id} id={selected.id} kind={selected} props={props} render={render} />
              </Section>
            ) : selection.length > 1 ? (
              <Section title={`${selection.length} items`}>
                <div className="btnrow">
                  <button type="button" className="btn" onClick={() => props.onDeleteItems(selection)}>
                    Delete
                  </button>
                </div>
              </Section>
            ) : null}

            <Section title="Paper" testId="paper">
              <Field label="Sheet name" value={setup.name} onCommit={(name) => name.trim() && put({ name: name.trim() })} />
              <label className="sp-field">
                <span>Size</span>
                <select
                  aria-label="Paper size"
                  value={setup.paper.kind}
                  onChange={(e) => {
                    const kind = e.currentTarget.value as PaperSizeKind;
                    if (kind === "custom") {
                      const [w, h] = paperSize({ ...setup, landscape: false });
                      put({ paper: { kind: "custom", width: w, height: h } });
                    } else put({ paper: { kind } });
                  }}
                >
                  {PAPERS.map((p) => (
                    <option key={p.kind} value={p.kind}>
                      {p.label}
                    </option>
                  ))}
                </select>
              </label>
              {setup.paper.kind === "custom" ? (
                <div className="sp-grid2">
                  <LengthField label="Width" value={setup.paper.width} units={units} onCommit={(width) => setup.paper.kind === "custom" && put({ paper: { ...setup.paper, width } })} />
                  <LengthField label="Height" value={setup.paper.height} units={units} onCommit={(height) => setup.paper.kind === "custom" && put({ paper: { ...setup.paper, height } })} />
                </div>
              ) : null}
              <div className="sp-seg" role="radiogroup" aria-label="Orientation">
                <button type="button" role="radio" aria-checked={!setup.landscape} onClick={() => put({ landscape: false })}>
                  Portrait
                </button>
                <button type="button" role="radio" aria-checked={!!setup.landscape} onClick={() => put({ landscape: true })}>
                  Landscape
                </button>
              </div>
              <label className="sp-field">
                <span>Units</span>
                <select aria-label="Paper units" value={units} onChange={(e) => put({ units: e.currentTarget.value as SheetSetup["units"] })}>
                  <option value="inches">Inches</option>
                  <option value="millimetres">Millimetres</option>
                </select>
              </label>
              <div className="sp-grid2">
                {(["Top", "Right", "Bottom", "Left"] as const).map((side, i) => (
                  <LengthField
                    key={side}
                    label={`${side} margin`}
                    value={(setup.margins ?? [0.0127, 0.0127, 0.0127, 0.0127])[i]}
                    units={units}
                    onCommit={(m) =>
                      put((s) => {
                        const margins = [...(s.margins ?? [0.0127, 0.0127, 0.0127, 0.0127])] as [number, number, number, number];
                        margins[i] = m;
                        return { margins };
                      })
                    }
                  />
                ))}
              </div>
              <LengthField label="Grid" value={setup.grid ?? 0.0254 / 8} units={units} onCommit={(grid) => put({ grid })} />
              <label className="sp-field">
                <span>Dimensions</span>
                <select
                  aria-label="Dimension format"
                  value={formatKey(setup.format ?? { kind: "fraction", den: 16 })}
                  onChange={(e) => {
                    const f = FORMATS.find((x) => formatKey(x.format) === e.currentTarget.value);
                    if (f) put({ format: f.format as LengthFormat });
                  }}
                >
                  {FORMATS.map((f) => (
                    <option key={f.label} value={formatKey(f.format)}>
                      {f.label}
                    </option>
                  ))}
                  {!FORMATS.some((f) => formatKey(f.format) === formatKey(setup.format ?? { kind: "fraction", den: 16 })) ? <option value={formatKey(setup.format!)}>Custom</option> : null}
                </select>
              </label>
            </Section>

            <Section title="Title block" testId="title-block">
              <label className="sp-check">
                <input type="checkbox" aria-label="Show the title block" checked={setup.title?.show !== false} onChange={(e) => title({ show: e.currentTarget.checked })} /> Show the title block
              </label>
              <Field label="Title" value={setup.title?.title ?? ""} placeholder={setup.name} onCommit={(v) => title({ title: v })} />
              <Field label="Project" value={setup.title?.project ?? ""} onCommit={(v) => title({ project: v })} />
              <div className="sp-grid2">
                <Field label="Sheet" value={setup.title?.sheet ?? ""} placeholder={`${sheets.findIndex((x) => x.id === row.id) + 1} of ${sheets.length}`} onCommit={(v) => title({ sheet: v })} />
                <Field label="Scale" value={setup.title?.scale ?? ""} placeholder="From the views" onCommit={(v) => title({ scale: v })} />
                <Field label="Date" value={setup.title?.date ?? ""} placeholder="YYYY-MM-DD" onCommit={(v) => title({ date: v })} />
                <Field label="Revision" value={setup.title?.revision ?? ""} onCommit={(v) => title({ revision: v })} />
              </div>
              <Field label="Drawn by" value={setup.title?.drawn_by ?? ""} onCommit={(v) => title({ drawn_by: v })} />
              <button type="button" className="btn" onClick={() => title({ date: localDate() })}>
                Today's date
              </button>
            </Section>

            <Section title="Print and export" testId="sheet-export">
              <div className="btnrow">
                {EXPORTS.filter((x) => props.exports.includes(x.kind)).map((x) => (
                  <button key={x.kind} type="button" className={x.primary ? "btn primary" : "btn"} data-export={x.kind} title={x.title} onClick={() => props.onExport(x.kind)}>
                    {x.label}
                  </button>
                ))}
              </div>
              <p className="note">At the paper's real size: {paperText(paperSize(setup)[0], units)} x {paperText(paperSize(setup)[1], units)}. Print at 100% (no fit to page) to keep the scales.</p>
            </Section>
          </>
        )}
      </section>
    </aside>
  );
}

export { sourceLabel };
