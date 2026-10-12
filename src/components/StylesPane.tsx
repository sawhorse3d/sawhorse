// The Styles pane (lane 10, ADR-0008): six presets, how faces are drawn (shaded with
// textures, shaded, monochrome, hidden line, wireframe, X-ray), edges (on, profiles,
// extensions, endpoints), the background, sky and ground colours, section cuts, planes, fill
// and the cut line (lane 12, ADR-0010), and the sun: shadows on or
// off, the date, the time, the latitude, light and dark. Sliders show their effect live and
// are one undo step when let go; styles and shadows are saved with the document and with
// each scene.
import { useEffect, useState } from "react";
import type { FaceStyle, Shadows, Style } from "../kernel/types";
import { DAYS_IN_MONTH, DEFAULT_SHADOWS, FACE_STYLES, MONTHS, STYLE_PRESETS, clock, compactStyle, fullStyle, hexRgb, rgbHex, type FullStyle } from "../lib/looks";

export interface StylesPaneProps {
  style: Style | null;
  shadows: Shadows | null;
  /** Commit a style (one undo step). */
  onStyle: (s: Style | null) => void;
  /** Commit shadows (one undo step). */
  onShadows: (s: Shadows) => void;
  /** Live preview while a slider moves (null: back to the document's). */
  onPreviewShadows: (s: Shadows | null) => void;
}

function Toggle({ label, checked, onChange, disabled, title }: { label: string; checked: boolean; onChange: (v: boolean) => void; disabled?: boolean; title?: string }) {
  return (
    <label className="chk" title={title}>
      <input type="checkbox" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
      {label}
    </label>
  );
}

function Colour({ label, value, fallback, onChange }: { label: string; value: [number, number, number] | null; fallback: string; onChange: (v: [number, number, number] | null) => void }) {
  return (
    <div className="colour-row">
      <label className="chk">
        <input type="checkbox" checked={!!value} aria-label={`${label} on`} onChange={(e) => onChange(e.target.checked ? hexRgb(fallback) : null)} />
        {label}
      </label>
      <input type="color" aria-label={`${label} colour`} disabled={!value} value={value ? rgbHex(value) : fallback} onChange={(e) => onChange(hexRgb(e.target.value))} />
    </div>
  );
}

export function StylesPane({ style, shadows, onStyle, onShadows, onPreviewShadows }: StylesPaneProps) {
  const s = fullStyle(style);
  const set = (next: Partial<FullStyle>) => onStyle(compactStyle({ ...s, preset: null, ...next }));
  const doc = shadows ?? DEFAULT_SHADOWS;
  // Slider values while dragging (committed when let go).
  const [draft, setDraft] = useState<Shadows>(doc);
  useEffect(() => setDraft(doc), [doc.on, doc.month, doc.day, doc.minutes, doc.latitude, doc.light, doc.dark, doc.ground, doc.faces]); // eslint-disable-line react-hooks/exhaustive-deps
  const live = (next: Partial<Shadows>) => {
    const d = { ...draft, ...next };
    setDraft(d);
    onPreviewShadows(d);
  };
  const commit = (next: Partial<Shadows> = {}) => {
    const d = { ...draft, ...next };
    if (d.day > DAYS_IN_MONTH[d.month - 1]) d.day = DAYS_IN_MONTH[d.month - 1];
    setDraft(d);
    onPreviewShadows(null);
    onShadows(d);
  };
  const slider = (key: "minutes" | "latitude" | "light" | "dark", min: number, max: number, step: number, label: string, shown: string) => (
    <label className="slider">
      <span>
        {label} <b>{shown}</b>
      </span>
      <input
        type="range"
        aria-label={label}
        min={min}
        max={max}
        step={step}
        value={draft[key]}
        disabled={!draft.on}
        onChange={(e) => live({ [key]: Number(e.target.value) })}
        onPointerUp={() => commit()}
        onKeyUp={() => commit()}
        onBlur={() => {
          if (draft[key] !== doc[key]) commit();
        }}
      />
    </label>
  );

  return (
    <div className="styles" data-testid="styles">
      <h4>Presets</h4>
      <div className="presets" role="group" aria-label="Style presets">
        {STYLE_PRESETS.map((p) => (
          <button key={p.name} type="button" aria-pressed={s.preset === p.name || (!style && p.name === "Sawhorse")} title={p.hint} onClick={() => onStyle(compactStyle(p.style))}>
            <span className={`preset-chip face-${p.style.face}`} aria-hidden="true"></span>
            {p.name}
          </button>
        ))}
      </div>

      <h4>Faces</h4>
      <div className="radios" role="radiogroup" aria-label="Face style">
        {FACE_STYLES.map((f) => (
          <label key={f.id} className="chk">
            <input type="radio" name="face-style" checked={s.face === f.id} onChange={() => set({ face: f.id as FaceStyle })} />
            {f.label}
          </label>
        ))}
      </div>

      <h4>Edges</h4>
      <div className="radios">
        <Toggle label="Edges" checked={s.edges.edges} onChange={(v) => set({ edges: { ...s.edges, edges: v } })} />
        <Toggle label="Profiles" checked={s.edges.profiles} disabled={!s.edges.edges} onChange={(v) => set({ edges: { ...s.edges, profiles: v } })} title="Outlines drawn heavier" />
        <Toggle label="Extensions" checked={s.edges.extensions} disabled={!s.edges.edges} onChange={(v) => set({ edges: { ...s.edges, extensions: v } })} title="Edges run a little past their ends" />
        <Toggle label="Endpoints" checked={s.edges.endpoints} disabled={!s.edges.edges} onChange={(v) => set({ edges: { ...s.edges, endpoints: v } })} title="The ends of edges drawn heavier" />
      </div>

      <h4>Background</h4>
      <Colour label="Background" value={s.background} fallback="#f4f5f1" onChange={(v) => set({ background: v })} />
      <Colour label="Sky" value={s.sky} fallback="#a8cae8" onChange={(v) => set({ sky: v })} />
      <Colour label="Ground" value={s.ground} fallback="#c4bca0" onChange={(v) => set({ ground: v })} />

      <h4>Sections</h4>
      <div className="radios" data-testid="section-style">
        <Toggle label="Section cuts" checked={s.section_cuts} title="Active section planes cut the model" onChange={(v) => set({ section_cuts: v })} />
        <Toggle label="Section planes" checked={s.section_planes} title="Draw the section planes themselves" onChange={(v) => set({ section_planes: v })} />
        <Toggle label="Section fill" checked={s.section_fill} disabled={!s.section_cuts} title="Fill the faces a section plane cuts" onChange={(v) => set({ section_fill: v })} />
      </div>
      <Colour label="Fill colour" value={s.section_fill_color} fallback="#a79e90" onChange={(v) => set({ section_fill_color: v })} />
      <Colour label="Cut line colour" value={s.section_line_color} fallback="#1f2421" onChange={(v) => set({ section_line_color: v })} />
      <label className="field-row">
        Cut line width
        <select aria-label="Cut line width" value={s.section_line_width} disabled={!s.section_cuts} onChange={(e) => set({ section_line_width: Number(e.target.value) })}>
          {Array.from({ length: 12 }, (_, i) => (
            <option key={i} value={i + 1}>
              {i + 1} px
            </option>
          ))}
        </select>
      </label>

      <h4>Shadows</h4>
      <div className="shadows" data-testid="shadows">
        <Toggle label="Show shadows" checked={draft.on} onChange={(v) => commit({ on: v })} />
        <div className="date-row">
          <label>
            Date
            <select aria-label="Month" value={draft.month} disabled={!draft.on} onChange={(e) => commit({ month: Number(e.target.value) })}>
              {MONTHS.map((m, i) => (
                <option key={m} value={i + 1}>
                  {m}
                </option>
              ))}
            </select>
          </label>
          <select aria-label="Day" value={draft.day} disabled={!draft.on} onChange={(e) => commit({ day: Number(e.target.value) })}>
            {Array.from({ length: DAYS_IN_MONTH[draft.month - 1] }, (_, i) => (
              <option key={i} value={i + 1}>
                {i + 1}
              </option>
            ))}
          </select>
        </div>
        {slider("minutes", 0, 1435, 5, "Time", clock(draft.minutes))}
        {slider("latitude", -66, 66, 1, "Latitude", `${Math.abs(draft.latitude)}° ${draft.latitude >= 0 ? "N" : "S"}`)}
        {slider("light", 0, 100, 1, "Light", String(draft.light))}
        {slider("dark", 0, 100, 1, "Dark", String(draft.dark))}
        <div className="radios">
          <Toggle label="On the ground" checked={draft.ground} disabled={!draft.on} onChange={(v) => commit({ ground: v })} />
          <Toggle label="On faces" checked={draft.faces} disabled={!draft.on} onChange={(v) => commit({ faces: v })} />
        </div>
        <p className="note">Solar time, north along green. Scenes save the style and the shadows with the camera.</p>
      </div>
    </div>
  );
}
