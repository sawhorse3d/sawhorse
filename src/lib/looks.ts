// Styles, shadows and materials on the UI side (lane 10, ADR-0008): the six style presets,
// the defaults, the sun's direction (the same formula as crates/kernel/src/looks.rs
// `Shadows::sun_direction`), and small helpers the panes and the Paint tool share.
import type { FaceStyle, LibraryEntry, MaterialInfo, SceneDef, Shadows, Style, UvXform } from "../kernel/types";

export const DEFAULT_SHADOWS: Shadows = { on: false, month: 6, day: 21, minutes: 13 * 60 + 30, latitude: 45, light: 80, dark: 45, ground: true, faces: true };

/** A style with every field filled in (what the renderer and the pane read). */
export interface FullStyle {
  preset: string | null;
  face: FaceStyle;
  edges: { edges: boolean; profiles: boolean; extensions: boolean; endpoints: boolean; soft: boolean };
  background: [number, number, number] | null;
  sky: [number, number, number] | null;
  ground: [number, number, number] | null;
  /** Section planes (lane 12): active planes cut, planes drawn, cut faces filled. */
  section_cuts: boolean;
  section_planes: boolean;
  section_fill: boolean;
  section_fill_color: [number, number, number] | null;
  section_line_color: [number, number, number] | null;
  /** Screen pixels. */
  section_line_width: number;
  /** Lane 21 (issue #4): the ground grid drawn (View > Grid). */
  grid: boolean;
}

/** The cut outline's width when the style does not say (screen pixels). */
export const DEFAULT_SECTION_LINE_WIDTH = 3;

export function fullStyle(s: Style | null | undefined): FullStyle {
  return {
    preset: s?.preset ?? null,
    face: s?.face ?? "textured",
    edges: {
      edges: s?.edges?.edges ?? true,
      profiles: s?.edges?.profiles ?? false,
      extensions: s?.edges?.extensions ?? false,
      endpoints: s?.edges?.endpoints ?? false,
      soft: s?.edges?.soft ?? false,
    },
    background: s?.background ?? null,
    sky: s?.sky ?? null,
    ground: s?.ground ?? null,
    section_cuts: s?.section_cuts ?? true,
    section_planes: s?.section_planes ?? true,
    section_fill: s?.section_fill ?? true,
    section_fill_color: s?.section_fill_color ?? null,
    section_line_color: s?.section_line_color ?? null,
    section_line_width: s?.section_line_width ?? DEFAULT_SECTION_LINE_WIDTH,
    grid: s?.grid ?? true,
  };
}

/** The style as the kernel stores it: defaults left out (the kernel drops a default style entirely). */
export function compactStyle(s: FullStyle): Style | null {
  const out: Style = {};
  if (s.preset) out.preset = s.preset;
  if (s.face !== "textured") out.face = s.face;
  const e = s.edges;
  if (!e.edges || e.profiles || e.extensions || e.endpoints || e.soft) {
    out.edges = {};
    if (!e.edges) out.edges.edges = false;
    if (e.profiles) out.edges.profiles = true;
    if (e.extensions) out.edges.extensions = true;
    if (e.endpoints) out.edges.endpoints = true;
    if (e.soft) out.edges.soft = true;
  }
  if (s.background) out.background = s.background;
  if (s.sky) out.sky = s.sky;
  if (s.ground) out.ground = s.ground;
  if (!s.section_cuts) out.section_cuts = false;
  if (!s.section_planes) out.section_planes = false;
  if (!s.section_fill) out.section_fill = false;
  if (s.section_fill_color) out.section_fill_color = s.section_fill_color;
  if (s.section_line_color) out.section_line_color = s.section_line_color;
  if (s.section_line_width !== DEFAULT_SECTION_LINE_WIDTH) out.section_line_width = Math.max(1, Math.min(12, Math.round(s.section_line_width)));
  if (!s.grid) out.grid = false;
  return Object.keys(out).length ? out : null;
}

/** Lane 21: whether the ground grid shows under a document's style (no style, or a style
 * without the switch: shown). What View > Grid's checkmark and the Styles pane read. */
export function gridShown(s: Style | null | undefined): boolean {
  return fullStyle(s).grid;
}

/** Lane 26 (issue #6): the grid that shows after going to scene `sc` from style `style`, as
 * the kernel's `activate_scene` sets it: the scene's own switch, else (a lane-10 scene) the
 * style it saved, else (older files, imported scenes) the grid as it is. */
export function gridAfterScene(sc: Pick<SceneDef, "style" | "grid">, style: Style | null | undefined): boolean {
  if (typeof sc.grid === "boolean") return sc.grid;
  return sc.style ? gridShown(sc.style) : gridShown(style);
}

export interface StylePreset {
  name: string;
  hint: string;
  style: FullStyle;
}

const base = (o: Omit<Partial<FullStyle>, "edges"> & { name: string; edges?: Partial<FullStyle["edges"]> }): FullStyle => ({ ...fullStyle(null), ...o, preset: o.name, edges: { ...fullStyle(null).edges, ...(o.edges ?? {}) } });

/** The six presets of the Styles pane. Ours: chosen for a woodworker's drawing, not copied from any program. */
export const STYLE_PRESETS: StylePreset[] = [
  { name: "Sawhorse", hint: "Materials with textures, plain edges, the theme's background.", style: { ...fullStyle(null), preset: "Sawhorse" } },
  { name: "Shop drawing", hint: "Hidden line on paper, heavy outlines, pencil overruns.", style: base({ name: "Shop drawing", face: "hidden_line", edges: { edges: true, profiles: true, extensions: true, endpoints: false }, background: [250, 248, 241] }) },
  { name: "Shaded", hint: "Material colours without textures, outlines heavier.", style: base({ name: "Shaded", face: "shaded", edges: { edges: true, profiles: true, extensions: false, endpoints: false } }) },
  { name: "X-ray", hint: "See-through faces: every edge, joinery inside included.", style: base({ name: "X-ray", face: "x_ray" }) },
  { name: "Clay", hint: "One colour for every face, outlines and endpoints heavier.", style: base({ name: "Clay", face: "monochrome", edges: { edges: true, profiles: true, extensions: false, endpoints: true } }) },
  { name: "Outdoors", hint: "Textures under a sky, on the ground, with heavy outlines.", style: base({ name: "Outdoors", face: "textured", edges: { edges: true, profiles: true, extensions: false, endpoints: false }, sky: [168, 202, 232], ground: [196, 188, 160], background: [226, 236, 244] }) },
];

export const FACE_STYLES: { id: FaceStyle; label: string }[] = [
  { id: "textured", label: "Shaded with textures" },
  { id: "shaded", label: "Shaded" },
  { id: "monochrome", label: "Monochrome" },
  { id: "hidden_line", label: "Hidden line" },
  { id: "wireframe", label: "Wireframe" },
  { id: "x_ray", label: "X-ray" },
];

const CUM = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
export const DAYS_IN_MONTH = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
export const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** Unit vector towards the sun (Z up, north +Y, east +X); z < 0 below the horizon. */
export function sunDirection(s: Pick<Shadows, "month" | "day" | "minutes" | "latitude">): [number, number, number] {
  const rad = Math.PI / 180;
  const n = CUM[Math.min(12, Math.max(1, s.month)) - 1] + s.day;
  const decl = 23.44 * rad * Math.sin((360 / 365) * (n - 81) * rad);
  const ha = (s.minutes / 60 - 12) * 15 * rad;
  const lat = s.latitude * rad;
  const alt = Math.asin(Math.min(1, Math.max(-1, Math.sin(lat) * Math.sin(decl) + Math.cos(lat) * Math.cos(decl) * Math.cos(ha))));
  const az = Math.atan2(-Math.sin(ha) * Math.cos(decl), Math.cos(lat) * Math.sin(decl) - Math.sin(lat) * Math.cos(decl) * Math.cos(ha));
  return [Math.cos(alt) * Math.sin(az), Math.cos(alt) * Math.cos(az), Math.sin(alt)];
}

/** "3:00 pm". */
export function clock(minutes: number): string {
  const h = Math.floor(minutes / 60) % 24;
  const m = minutes % 60;
  const h12 = h % 12 === 0 ? 12 : h % 12;
  return `${h12}:${String(m).padStart(2, "0")} ${h < 12 ? "am" : "pm"}`;
}

export function rgbHex(c: readonly number[]): string {
  return `#${c
    .slice(0, 3)
    .map((v) => Math.round(v).toString(16).padStart(2, "0"))
    .join("")}`;
}

export function hexRgb(h: string): [number, number, number] {
  const v = parseInt(h.replace("#", ""), 16);
  return [(v >> 16) & 255, (v >> 8) & 255, v & 255];
}

/** The positioning a face has after `next` changes on top of `uv` (normalised, defaults dropped). */
export function cleanUv(uv: UvXform): UvXform | null {
  const scale = uv.scale ?? [1, 1];
  const rotation = (((uv.rotation ?? 0) % 360) + 360) % 360;
  const offset = uv.offset ?? [0, 0];
  const out: UvXform = {};
  if (scale[0] !== 1 || scale[1] !== 1) out.scale = scale;
  if (rotation !== 0) out.rotation = rotation;
  if (offset[0] !== 0 || offset[1] !== 0) out.offset = offset;
  return Object.keys(out).length ? out : null;
}

/** A name for a new material from an image file name: "white oak.jpg" -> "White oak", unique among `taken`. */
export function materialNameFor(file: string, taken: string[]): string {
  const stem = file.replace(/\.[^.]+$/, "").replace(/[_-]+/g, " ").trim() || "Material";
  const nice = (stem.charAt(0).toUpperCase() + stem.slice(1)).slice(0, 56);
  if (!taken.includes(nice)) return nice;
  for (let k = 2; ; k++) if (!taken.includes(`${nice} ${k}`)) return `${nice} ${k}`;
}

/** A texture path the kernel accepts for an image file: assets/<slug>-<n>.png|.jpg. */
export function texturePathFor(file: string, mime: string, taken: string[]): string {
  const ext = mime === "image/jpeg" ? "jpg" : "png";
  const slug =
    file
      .replace(/\.[^.]+$/, "")
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 40) || "image";
  let path = `assets/${slug}.${ext}`;
  for (let k = 2; taken.includes(path); k++) path = `assets/${slug}-${k}.${ext}`;
  return path;
}

/** The image type from the first bytes (PNG or JPEG), or null. */
export function sniffImage(bytes: Uint8Array): "image/png" | "image/jpeg" | null {
  if (bytes.length > 8 && bytes[0] === 0x89 && bytes[1] === 0x50 && bytes[2] === 0x4e && bytes[3] === 0x47) return "image/png";
  if (bytes.length > 3 && bytes[0] === 0xff && bytes[1] === 0xd8) return "image/jpeg";
  return null;
}

/** Base64 of bytes (the add_texture op carries images as base64). */
export function toBase64(bytes: Uint8Array): string {
  let s = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) s += String.fromCharCode(...bytes.subarray(i, i + chunk));
  return btoa(s);
}

/** What the Paint tool paints with: a material in the model, or one from the library (added on first use). */
export type ActiveMaterial = { from: "model"; name: string } | { from: "library"; name: string } | { from: "none" };

/** The material named `name` in the model, if any. */
export function modelMaterial(materials: MaterialInfo[] | undefined, name: string): MaterialInfo | undefined {
  return materials?.find((m) => m.name === name);
}

/** Library entries matching a search (name, category or species). */
export function searchLibrary(lib: LibraryEntry[], q: string, category: string | null): LibraryEntry[] {
  const t = q.trim().toLowerCase();
  return lib.filter((e) => (!category || e.category === category) && (!t || e.name.toLowerCase().includes(t) || e.category.toLowerCase().includes(t) || (e.stock?.species ?? "").toLowerCase().includes(t)));
}
