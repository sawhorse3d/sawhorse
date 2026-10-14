// three.js viewport (WebGL2). Z-up, metres, exactly the kernel's frame.
//
// Rendering is on demand (camera change, mesh change, theme change, resize); there is no
// animation loop burning a laptop battery while nothing moves.
//
// Navigation (the left button belongs to the active tool):
//   middle-drag orbits, Shift+middle-drag pans, right-drag pans, the wheel zooms toward the
//   cursor; Shift+Z (zoom extents), the standard views and the projection toggle are
//   driven from the app.
//
// Layers, back to front: ground grid (no depth writes, drawn first and nudged a hair below
// z = 0, so sketch regions on the ground always draw over it), axes, model faces (pushed
// back by a polygon offset so edges draw on top), model edges, highlights, overlays (tool
// previews and inference lines, drawn without depth test so they are never hidden).
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { LineSegments2 } from "three/examples/jsm/lines/LineSegments2.js";
import { LineSegmentsGeometry } from "three/examples/jsm/lines/LineSegmentsGeometry.js";
import { LineMaterial } from "three/examples/jsm/lines/LineMaterial.js";
import type { GraphSection, MaterialInfo, MeshData, SectionView, Shadows, Style, TextureBlob, TextureInfo } from "../kernel/types";
import { ownerEdgeCount, ownerSegments } from "../lib/meshIndex";
import { clippedEdges, partClipPlanes, pickSection, sectionArrows, sectionCorners } from "../lib/sections";
import { fullStyle, sunDirection, type FullStyle } from "../lib/looks";
import { TextureCache, edgeInk, edgeSets, gridTones, endpointSegments, extendedSegments, faceMode, makeBackdrop, profileSegments, rgbCss, setBackdrop, toneWeights, type EdgeSets } from "./looks";
import type { UnitSystem } from "../lib/units";
import {
  AXIS_COLORS,
  PERSPECTIVE_FOV_DEG,
  VIEW_DIRS,
  equivalentOrthoHalfHeight,
  equivalentPerspectiveDistance,
  gridHalfExtent,
  gridLines,
  gridSpec,
  gridVisibility,
  projectionForView,
  type GridSpec,
  type PixelScale,
  type Projection,
  type Vec3,
  type ViewId,
} from "./camera";

export { VIEWS, type ViewId, type Projection } from "./camera";

/** Nudge the grid below the ground so coplanar regions win the depth test (lane-2 open question 3). */
export const GRID_DEPTH_OFFSET = -5e-5;

/** The default back-face colour: the inner sides of imported meshes that are not closed. */
export const BACK_FACE_COLOUR = "#7f93ad";

interface Theme {
  /** Section planes (lane 12): the active plane's colour and the default cut fill. */
  section: string;
  sectionFill: string;
  vp: string;
  grid: string;
  gridMajor: string;
  face: string;
  faceTop: string;
  faceSide: string;
  edge: string;
  sel: string;
  hover: string;
}

/** A line overlay: tool previews, inference constraint lines, guides being placed. */
export interface OverlayItem {
  /** "segments": pairs of points; "strip": a polyline; "loop": a closed polyline. */
  kind: "segments" | "strip" | "loop";
  points: Vec3[];
  /** CSS colour, or a CSS custom property name such as "--sel". */
  color: string;
  /** Pixel width (fat lines). */
  width?: number;
  dashed?: boolean;
  /** Hidden behind geometry (default false: overlays always show). */
  depthTest?: boolean;
}

/** Highlighted model entities (indices into MeshData.parts; edges as world segments). */
export interface Highlight {
  hoverParts: number[];
  selectedParts: number[];
  hoverEdges: [Vec3, Vec3][];
  selectedEdges: [Vec3, Vec3][];
  selectedPoints: Vec3[];
  /** Parts outside the open group or component, drawn faded (lane 5). */
  fadedParts?: number[];
  /** Section planes selected and under the cursor (lane 12). */
  sections?: { selected: number[]; hover: number | null };
  /** Lane 16b: whole solids whose every edge is highlighted, as indices into
   * `MeshData.edgeOwners` (the viewport draws their segments; see FAT_HIGHLIGHT_EDGES). */
  selectedEdgeOwners?: number[];
  hoverEdgeOwners?: number[];
}

export const NO_HIGHLIGHT: Highlight = { hoverParts: [], selectedParts: [], hoverEdges: [], selectedEdges: [], selectedPoints: [] };

/**
 * Lane 16b: the most edge segments of whole solids drawn as fat (several-pixel) highlight
 * lines. Past it, the edges are drawn in the highlight colour as the view's own thin lines,
 * picked out on the GPU from a per-solid state texture. Selecting all of 100,000 solids
 * built 1,200,000 fat lines on the window's thread and then drew them every frame: seconds
 * a frame where the GPU is emulated in software (CI runners), and the face tint was a pass
 * over every triangle. Now a selection costs what it holds: a byte per part and per solid
 * written to two textures, and the same frame as before it.
 */
export const FAT_HIGHLIGHT_EDGES = 20_000;

/** Highlight state bits, one byte per part (faces) and per solid (edges). */
const SELECTED = 1;
const HOVERED = 2;
const FADED = 4;
const NONE: readonly number[] = [];

/** One byte per item, read by the shaders with texelFetch (row-major, `width` a row). */
class StateTexture {
  readonly data: Uint8Array;
  readonly texture: THREE.DataTexture;
  readonly width: number;
  readonly count: number;
  constructor(count: number, maxSize: number) {
    this.count = count;
    this.width = Math.max(1, Math.min(4096, maxSize));
    const rows = Math.max(1, Math.ceil(count / this.width));
    this.data = new Uint8Array(this.width * rows);
    this.texture = new THREE.DataTexture(this.data, this.width, rows, THREE.RedFormat, THREE.UnsignedByteType);
    this.texture.minFilter = THREE.NearestFilter;
    this.texture.magFilter = THREE.NearestFilter;
    this.texture.generateMipmaps = false;
    this.texture.needsUpdate = true;
  }
  /** Set or clear `bit` on every item of `list`; true when anything was listed. */
  mark(list: readonly number[], bit: number, on: boolean): boolean {
    if (!list.length) return false;
    const d = this.data;
    const n = this.count;
    const off = ~bit & 0xff;
    if (on) {
      for (const i of list) if (i < n) d[i] |= bit;
    } else {
      for (const i of list) if (i < n) d[i] &= off;
    }
    this.texture.needsUpdate = true;
    return true;
  }
  dispose(): void {
    this.texture.dispose();
  }
}

/** GLSL: the state byte of item `p` in a StateTexture. */
const STATE_FETCH = `int sawhorseState(highp sampler2D t, int w, int p) {
  return int(texelFetch(t, ivec2(p - (p / w) * w, p / w), 0).r * 255.0 + 0.5);
}`;

function readTheme(el: Element): Theme {
  const cs = getComputedStyle(el);
  const v = (n: string, fallback: string) => cs.getPropertyValue(n).trim() || fallback;
  return {
    section: v("--section", "#c4622d"),
    sectionFill: v("--section-fill", "#a79e90"),
    vp: v("--vp", "#f4f5f1"),
    grid: v("--vp-grid", "#d4d8cf"),
    gridMajor: v("--vp-axis", "#9aa39a"),
    face: v("--face", "#e9e4d6"),
    faceTop: v("--face-top", "#f3efe3"),
    faceSide: v("--face-side", "#d9d2c0"),
    edge: v("--edge", "#3a3f3b"),
    sel: v("--sel", "#2a63a8"),
    hover: v("--accent", "#2a63a8"),
  };
}

export function webgl2Available(): boolean {
  try {
    const c = document.createElement("canvas");
    return !!c.getContext("webgl2");
  } catch {
    return false;
  }
}

function disposeObject(o: THREE.Object3D | null): void {
  if (!o) return;
  o.traverse((c) => {
    const m = c as THREE.Mesh;
    m.geometry?.dispose?.();
    const mat = m.material as THREE.Material | THREE.Material[] | undefined;
    if (Array.isArray(mat)) mat.forEach((x) => x.dispose());
    else mat?.dispose?.();
  });
  o.removeFromParent();
}

/** Each render vertex's part (the faces' shader looks its highlight state up by it). */
function partOfVertex(mesh: MeshData, vertices: number): Float32Array {
  const out = new Float32Array(vertices);
  const { indices, faceIds } = mesh;
  for (let t = 0; t < faceIds.length; t++) {
    const part = faceIds[t];
    out[indices[3 * t]] = part;
    out[indices[3 * t + 1]] = part;
    out[indices[3 * t + 2]] = part;
  }
  return out;
}

/** Segments as six floats each. */
function flatPairs(pairs: Vec3[][]): Float32Array {
  const out = new Float32Array(pairs.length * 6);
  pairs.forEach(([a, b], i) => {
    out.set(a, 6 * i);
    out.set(b, 6 * i + 3);
  });
  return out;
}

export class ViewportEngine {
  readonly renderer: THREE.WebGLRenderer;
  private scene = new THREE.Scene();
  private ortho = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.01, 1000);
  private persp = new THREE.PerspectiveCamera(PERSPECTIVE_FOV_DEG, 1, 0.01, 1000);
  private camera: THREE.OrthographicCamera | THREE.PerspectiveCamera = this.ortho;
  private controls: OrbitControls;
  private gridMinor: THREE.LineSegments | null = null;
  private gridMajor: THREE.LineSegments | null = null;
  private axes = new THREE.Group();
  private faces: THREE.Mesh | null = null;
  /** Back sides of imported non-solid meshes, drawn in their own tone so an open shell reads as open. */
  private backs: THREE.Mesh | null = null;
  private edges: THREE.LineSegments | null = null;
  private meshData: MeshData | null = null;
  private highlight: Highlight = NO_HIGHLIGHT;
  private highlightGroup = new THREE.Group();
  private overlayGroup = new THREE.Group();
  private overlays: OverlayItem[] = [];
  /** Content the camera was last fitted to; the view refits only when content outgrows it. */
  private fitted: THREE.Box3 | null = null;
  private raycaster = new THREE.Raycaster();
  private theme: Theme;
  private view: ViewId | null = "iso";
  private projection: Projection = "parallel";
  private spec: GridSpec = gridSpec("frac-in");
  private gridHalf = 0;
  /** Half-height of the ortho frustum at zoom 1, metres. */
  private frame = 1;
  private size = { w: 1, h: 1 };
  private queued = false;
  private disposed = false;
  private listeners = new Set<() => void>();
  // Lane 10: materials, textures, the style and the sun.
  private style: FullStyle = fullStyle(null);
  private shadows: Shadows | null = null;
  private materials = new Map<string, MaterialInfo>();
  /** Texture path -> content digest (from the document state). */
  private texDigest = new Map<string, string>();
  private textures: TextureCache;
  private looksKey = "";
  /** Meshes drawing textured faces over the faces, one per image. */
  private textured = new THREE.Group();
  private texturedParts = 0;
  private paintPreview: { parts: number[]; material: MaterialInfo } | null = null;
  private previewParts = new Set<number>();
  /** Profiles and endpoints (fat lines), rebuilt when the camera moves. */
  private styledEdges = new THREE.Group();
  private styledEdgesKey = "";
  /** The mesh's edges by how they are drawn (lane 15: soft and hidden edges apart). */
  private edgeSet: EdgeSets | null = null;
  private backdrop = makeBackdrop();
  private sunLight = new THREE.DirectionalLight(0xffffff, 2);
  private ambient = new THREE.AmbientLight(0xffffff, 1);
  private groundShadow = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), new THREE.ShadowMaterial({ opacity: 0.3, depthWrite: false }));
  // Section planes (lane 12): the cuts to draw, the planes, and what draws them.
  private sectionView: SectionView | null = null;
  private sectionPlanes: GraphSection[] = [];
  private sectionGroup = new THREE.Group();
  private sectionKey = "";
  // Lane 16b: highlight state on the GPU. The faces' shader tints a part by its byte in
  // `partState`; `ownerLines` draws the edges of solids whose byte in `ownerState` is set
  // (only when a highlight holds more than FAT_HIGHLIGHT_EDGES segments). `applied` is what
  // the textures hold now, so a change writes only what it changes.
  private partState: StateTexture | null = null;
  private ownerState: StateTexture | null = null;
  private ownerLines: THREE.LineSegments | null = null;
  private applied = { selected: NONE, hover: NONE, faded: NONE, selOwners: NONE, hoverOwners: NONE };
  private stateUniforms = {
    uPartState: { value: null as THREE.Texture | null },
    uPartW: { value: 1 },
    uSelTint: { value: new THREE.Color() },
    uFadeTo: { value: new THREE.Color() },
  };
  private ownerUniforms = {
    uOwnerState: { value: null as THREE.Texture | null },
    uOwnerW: { value: 1 },
    uOwnerSel: { value: new THREE.Color() },
    uOwnerHover: { value: new THREE.Color() },
  };

  constructor(
    canvas: HTMLCanvasElement,
    private themeRoot: Element = document.documentElement,
  ) {
    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, preserveDrawingBuffer: true });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.theme = readTheme(themeRoot);
    this.scene.add(this.axes, this.highlightGroup, this.overlayGroup, this.textured, this.styledEdges, this.backdrop, this.sectionGroup);
    // Section planes clip per solid (each material carries its own planes).
    this.renderer.localClippingEnabled = true;
    this.textures = new TextureCache(this.renderer.capabilities.getMaxAnisotropy(), () => {
      // An image finished decoding: draw the faces that show it.
      if (this.faces) this.colourFaces(this.faces.geometry);
      this.buildOverlays();
      this.requestRender();
    });
    // PCF (three 0.186 removed PCFSoft and fell back to PCF with a console warning each
    // time shadows were first drawn; lane 15 asks for PCF directly: the same picture).
    this.renderer.shadowMap.type = THREE.PCFShadowMap;
    this.sunLight.shadow.mapSize.set(2048, 2048);
    this.groundShadow.receiveShadow = true;
    this.groundShadow.renderOrder = -1;
    this.groundShadow.frustumCulled = false;
    this.scene.add(this.sunLight, this.sunLight.target, this.ambient, this.groundShadow);
    this.sunLight.visible = false;
    this.ambient.visible = false;
    this.groundShadow.visible = false;

    this.controls = new OrbitControls(this.camera, canvas);
    // Left button belongs to the active tool. Middle orbits (Shift+middle pans: OrbitControls
    // turns a rotate gesture into a pan while Shift is held), right pans, wheel zooms.
    this.controls.mouseButtons = { LEFT: null, MIDDLE: THREE.MOUSE.ROTATE, RIGHT: THREE.MOUSE.PAN };
    this.controls.zoomToCursor = true;
    this.controls.enableDamping = false;
    this.controls.screenSpacePanning = true;
    this.controls.addEventListener("change", () => {
      // Any orbit leaves the standard view; a pure zoom or pan keeps the direction.
      if (this.view) {
        const want = new THREE.Vector3(...VIEW_DIRS[this.view].dir);
        const have = this.camera.position.clone().sub(this.controls.target).normalize();
        if (have.dot(want) < 0.9999) this.view = null;
      }
      this.updateGridVisibility();
      this.requestRender();
    });

    this.rebuildGrid();
    this.buildAxes();
    this.applyTheme();
    this.setView("iso");
  }

  // ---- grid and axes ---------------------------------------------------------------------

  /** Grid lines follow the display units (1 in / 1 ft, or 10 mm / 100 mm). */
  setUnits(units: UnitSystem): void {
    const spec = gridSpec(units);
    if (spec.minor === this.spec.minor && this.gridMinor) return;
    this.spec = spec;
    this.rebuildGrid();
    this.requestRender();
  }

  gridSpec(): GridSpec {
    return this.spec;
  }

  private rebuildGrid(): void {
    disposeObject(this.gridMinor);
    disposeObject(this.gridMajor);
    const box = this.contentBox();
    const r = box.getBoundingSphere(new THREE.Sphere()).radius + box.getCenter(new THREE.Vector3()).length();
    this.gridHalf = gridHalfExtent(this.spec, r);
    const mk = (pts: Float32Array, color: string, order: number) => {
      const g = new THREE.BufferGeometry();
      g.setAttribute("position", new THREE.BufferAttribute(pts, 3));
      const mat = new THREE.LineBasicMaterial({ color, transparent: true, depthWrite: false });
      const l = new THREE.LineSegments(g, mat);
      l.renderOrder = order;
      l.position.z = GRID_DEPTH_OFFSET;
      l.frustumCulled = false;
      return l;
    };
    const tones = gridTones(this.style.background, this.theme);
    this.gridMinor = mk(gridLines(this.spec.minor, this.gridHalf, this.spec.ratio), tones.minor, -2);
    this.gridMajor = mk(gridLines(this.spec.major, this.gridHalf), tones.major, -1);
    (this.gridMajor.material as THREE.LineBasicMaterial).opacity = 0.55;
    this.scene.add(this.gridMinor, this.gridMajor);
    this.updateGridVisibility();
  }

  private buildAxes(): void {
    this.axes.clear();
    const L = Math.max(this.gridHalf, 1);
    const axis = (dir: Vec3, color: string, negative: boolean) => {
      const s = negative ? -1 : 1;
      const pts = [0, 0, 0, dir[0] * L * s, dir[1] * L * s, dir[2] * L * s];
      const g = new THREE.BufferGeometry();
      g.setAttribute("position", new THREE.Float32BufferAttribute(pts, 3));
      const mat = negative
        ? new THREE.LineDashedMaterial({ color, dashSize: 0.02, gapSize: 0.02, depthWrite: false, transparent: true, opacity: 0.6 })
        : new THREE.LineBasicMaterial({ color, depthWrite: false });
      const line = new THREE.Line(g, mat);
      if (negative) line.computeLineDistances();
      line.renderOrder = 0;
      line.frustumCulled = false;
      return line;
    };
    for (const [dir, c] of [
      [[1, 0, 0], AXIS_COLORS.x],
      [[0, 1, 0], AXIS_COLORS.y],
      [[0, 0, 1], AXIS_COLORS.z],
    ] as [Vec3, string][]) {
      this.axes.add(axis(dir, c, false), axis(dir, c, true));
    }
  }

  /** Metres per CSS pixel at the orbit target (for the grid density and the HUD). */
  private pixelAtTarget(): number {
    return this.metresPerPixelAt([this.controls.target.x, this.controls.target.y, this.controls.target.z]);
  }

  private updateGridVisibility(): void {
    if (!this.gridMinor || !this.gridMajor) return;
    const minorPx = this.spec.minor / Math.max(this.pixelAtTarget(), 1e-9);
    const vis = gridVisibility(minorPx, this.spec.ratio);
    this.gridMinor.visible = vis.minor;
    (this.gridMinor.material as THREE.LineBasicMaterial).opacity = vis.opacity;
    this.gridMajor.visible = vis.major;
  }

  /** Which grid levels show right now (tests and the HUD). */
  gridState(): { minor: boolean; major: boolean; spec: GridSpec } {
    return { minor: !!this.gridMinor?.visible, major: !!this.gridMajor?.visible, spec: this.spec };
  }

  // ---- theme -------------------------------------------------------------------------------

  /** Re-read CSS tokens (called on theme switches). */
  applyTheme(): void {
    this.theme = readTheme(this.themeRoot);
    this.renderer.setClearColor(new THREE.Color(this.theme.vp), 1);
    this.colourGrid();
    this.stateUniforms.uSelTint.value.set(this.theme.sel);
    this.stateUniforms.uFadeTo.value.set(this.theme.vp);
    this.ownerUniforms.uOwnerSel.value.set(this.cssColor(this.theme.sel));
    this.ownerUniforms.uOwnerHover.value.set(this.cssColor(this.theme.hover));
    if (this.edges) (this.edges.material as THREE.LineBasicMaterial).color.set(this.ink());
    if (this.faces) this.colourFaces(this.faces.geometry);
    this.buildOverlays();
    this.styledEdgesKey = "";
    this.rebuildHighlights();
    this.rebuildOverlays();
    this.requestRender();
  }

  private cssColor(c: string): string {
    if (c.startsWith("--")) {
      return getComputedStyle(this.themeRoot).getPropertyValue(c).trim() || "#2a63a8";
    }
    return c;
  }

  // ---- model -----------------------------------------------------------------------------

  /** The material a part shows, as the style draws it: null for the theme's tones. */
  private partMaterial(part: number): MaterialInfo | null {
    const info = this.meshData?.parts[part];
    const preview = this.paintPreview;
    if (preview && this.previewParts.has(part)) return preview.material;
    if (!info?.material) return null;
    return this.materials.get(info.material) ?? null;
  }

  /** The texture a part shows in this style, when its image is loaded. */
  private partTexture(part: number): { digest: string; texture: THREE.Texture } | null {
    if (!faceMode(this.style).textures) return null;
    if (this.paintPreview && this.previewParts.has(part)) return null;
    const m = this.partMaterial(part);
    if (!m?.texture || !m.held) return null;
    const digest = this.texDigest.get(m.texture);
    const texture = this.textures.get(digest);
    return digest && texture ? { digest, texture } : null;
  }

  /** Colour each triangle: the mock's three face tones (top / front / side) or the material
   * colour shaded the same way, white under a texture (the image is the colour), the
   * background colour in hidden line. Under the sun the tones come from the light instead.
   * Lane 16b: the selection and hover tints and the fade outside the open group are not baked
   * in here (that was a pass over every triangle per selection change): the faces' shader
   * applies them from the part state texture (withPartState). */
  private colourFaces(geom: THREE.BufferGeometry): void {
    const pos = geom.getAttribute("position") as THREE.BufferAttribute;
    const index = geom.getIndex();
    if (!index) return;
    const colors = new Float32Array(pos.count * 3);
    const top = new THREE.Color(this.theme.faceTop);
    const front = new THREE.Color(this.theme.face);
    const side = new THREE.Color(this.theme.faceSide);
    const warn = new THREE.Color("#d9a33a");
    const paper = new THREE.Color(this.theme.vp);
    const mode = faceMode(this.style);
    const lit = this.lit();
    const background = this.style.background ? new THREE.Color(rgbCss(this.style.background)) : paper;
    const white = new THREE.Color(1, 1, 1);
    const ids = this.meshData?.faceIds;
    const a = new THREE.Vector3();
    const b = new THREE.Vector3();
    const c = new THREE.Vector3();
    const n = new THREE.Vector3();
    // Lane 15: across smooth edges each vertex has its own normal, so the theme's three tones
    // blend across a smoothed surface instead of stepping face by face.
    const vn = this.meshData?.normals;
    const smooth = !!vn && vn.length === pos.count * 3 && vn.length > 0;
    const blend = (w: [number, number, number]) =>
      new THREE.Color(top.r * w[0] + front.r * w[1] + side.r * w[2], top.g * w[0] + front.g * w[1] + side.g * w[2], top.b * w[0] + front.b * w[1] + side.b * w[2]);
    const reshade = (col: THREE.Color, flat: number, w: [number, number, number]) => col.clone().multiplyScalar((w[0] + 0.86 * w[1] + 0.72 * w[2]) / flat);
    for (let t = 0; t < index.count; t += 3) {
      const ia = index.getX(t);
      const ib = index.getX(t + 1);
      const ic = index.getX(t + 2);
      a.fromBufferAttribute(pos, ia);
      b.fromBufferAttribute(pos, ib);
      c.fromBufferAttribute(pos, ic);
      n.subVectors(b, a).cross(c.sub(a)).normalize();
      const part = ids ? ids[t / 3] : -1;
      const kind = n.z > 0.5 ? 0 : n.z < -0.5 ? 2 : Math.abs(n.y) >= Math.abs(n.x) ? 1 : 2;
      const shade = lit ? 1 : [1, 0.86, 0.72][kind];
      let col = lit ? new THREE.Color(this.theme.face) : [top, front, side][kind];
      const info = part >= 0 ? this.meshData?.parts[part] : undefined;
      const mat = part >= 0 ? this.partMaterial(part) : null;
      if (mode.paper) {
        col = background;
      } else if (part >= 0 && this.partTexture(part)) {
        // The texture is the colour; the vertex colour only shades it.
        col = white.clone().multiplyScalar(shade);
      } else if (mode.colours && mat) {
        // A material colour, shaded like the theme's three face tones.
        const [r, g, bb] = mat.color;
        col = new THREE.Color().setRGB(r / 255, g / 255, bb / 255, THREE.SRGBColorSpace).multiplyScalar(shade);
      } else if (mode.colours && !mat && info?.color) {
        const [r, g, bb] = info.color;
        col = new THREE.Color().setRGB(r / 255, g / 255, bb / 255, THREE.SRGBColorSpace).multiplyScalar(shade);
      }
      // Imported geometry that is not a solid reads amber-tinted: it cannot be pushed or printed.
      if (info?.kind === "mesh" && !mode.paper) col = col.clone().lerp(warn, 0.3);
      if (!smooth || lit || mode.paper) {
        for (const i of [ia, ib, ic]) col.toArray(colors, i * 3);
        continue;
      }
      // Smoothed: each vertex its own tone (the same as flat on a face whose normals are its own).
      const themed = !(part >= 0 && this.partTexture(part)) && !(mode.colours && (mat || info?.color));
      for (const i of [ia, ib, ic]) {
        const w = toneWeights(vn![3 * i], vn![3 * i + 1], vn![3 * i + 2]);
        let v = themed ? blend(w) : reshade(col, shade, w);
        if (themed && info?.kind === "mesh") v = v.lerp(warn, 0.3);
        v.toArray(colors, i * 3);
      }
    }
    geom.setAttribute("color", new THREE.BufferAttribute(colors, 3));
  }

  /** Shadows are being drawn (the faces are lit by the sun then). */
  private lit(): boolean {
    return !!this.shadows?.on && faceMode(this.style).faces && !faceMode(this.style).paper;
  }

  /**
   * Materials, textures, the style and the sun (lane 10). `materials` and `textures` come
   * from the document state; `blobs` are images new to this UI (decoded once, by digest).
   */
  setLooks(looks: { materials?: MaterialInfo[]; textures?: TextureInfo[]; style?: Style | null; shadows?: Shadows | null; blobs?: TextureBlob[] }): void {
    // Images the model no longer holds are let go first, so their pixels do not count
    // against the ones that come in (lane 16).
    if (looks.textures) this.textures.keep(looks.textures.map((t) => t.digest));
    for (const b of looks.blobs ?? []) this.textures.add(b.digest, b.mime, b.bytes);
    this.materials = new Map((looks.materials ?? []).map((m) => [m.name, m]));
    this.texDigest = new Map((looks.textures ?? []).map((t) => [t.path, t.digest]));
    const style = fullStyle(looks.style ?? null);
    const shadows = looks.shadows ?? null;
    const key = JSON.stringify([style, shadows, looks.materials, looks.textures]);
    if (key === this.looksKey) return;
    this.looksKey = key;
    this.style = style;
    this.shadows = shadows;
    this.colourGrid();
    this.buildFaces();
    this.requestRender();
  }

  /** The colour edges are drawn in (lane 15): the theme's, unless the style sets a
   * background it would not show on (the Shop drawing's paper in the dark theme): then a
   * dark ink on a light background, a light one on a dark background. */
  private ink(): string {
    return edgeInk(this.style.background, this.theme.edge);
  }

  /** The grid's colours follow the style's background when it sets one (lane 15). */
  private colourGrid(): void {
    const tones = gridTones(this.style.background, this.theme);
    if (this.gridMinor) (this.gridMinor.material as THREE.LineBasicMaterial).color.set(tones.minor);
    if (this.gridMajor) (this.gridMajor.material as THREE.LineBasicMaterial).color.set(tones.major);
  }

  /** The grid's line colours now (tests and captures). */
  gridColours(): { minor: string; major: string } {
    const hex = (l: THREE.LineSegments | null) => (l ? `#${(l.material as THREE.LineBasicMaterial).color.getHexString()}` : "");
    return { minor: hex(this.gridMinor), major: hex(this.gridMajor) };
  }

  /** What the style says right now (tests and captures). */
  looksState(): { face: string; profiles: boolean; shadows: boolean; textures: number; decoded: ReturnType<TextureCache["stats"]>; texturedParts: number; sun: Vec3 | null } {
    return {
      face: this.style.face,
      profiles: this.style.edges.profiles,
      shadows: this.lit(),
      textures: this.textures.size(),
      decoded: this.textures.stats(),
      texturedParts: this.texturedParts,
      sun: this.lit() && this.shadows ? sunDirection(this.shadows) : null,
    };
  }

  /** Show the Paint tool's hover: `parts` drawn in `material` (null: none). */
  setPaintPreview(p: { parts: number[]; material: MaterialInfo } | null): void {
    const same = (!p && !this.paintPreview) || (p && this.paintPreview && p.material.name === this.paintPreview.material.name && p.parts.join() === this.paintPreview.parts.join());
    if (same) return;
    this.paintPreview = p;
    this.previewParts = new Set(p?.parts ?? []);
    if (this.faces) {
      this.colourFaces(this.faces.geometry);
      this.buildOverlays();
    }
    this.requestRender();
  }

  /** Texture coordinates of one part replaced (the texture handles' live preview). */
  setPartUvs(part: number, uvs: Float32Array): void {
    const m = this.meshData;
    const g = this.faces?.geometry;
    const attr = g?.getAttribute("uv") as THREE.BufferAttribute | undefined;
    if (!m || !g || !attr || !uvs.length) return;
    let first = Infinity;
    for (let t = 0; t < m.faceIds.length; t++) {
      if (m.faceIds[t] !== part) continue;
      for (let k = 0; k < 3; k++) first = Math.min(first, m.indices[3 * t + k]);
    }
    if (!Number.isFinite(first) || (first + uvs.length / 2) * 2 > attr.array.length) return;
    (attr.array as Float32Array).set(uvs, first * 2);
    attr.needsUpdate = true;
    this.buildOverlays();
    this.requestRender();
  }

  setMesh(mesh: MeshData | null): void {
    this.meshData = mesh;
    // A hover preview names parts of the old mesh: it goes (the next pointer move sets it again).
    this.paintPreview = null;
    this.previewParts = new Set();
    this.buildFaces();
    // The grid grows with the model.
    const box = this.contentBox();
    const r = box.getBoundingSphere(new THREE.Sphere()).radius + box.getCenter(new THREE.Vector3()).length();
    if (gridHalfExtent(this.spec, r) > this.gridHalf) {
      this.rebuildGrid();
      this.buildAxes();
    }
    // Refit only when the model outgrows what the camera was framed for, and then only
    // outward (the working area and what was framed before stay in view), so drawing a
    // small shape never zooms the camera in on it.
    if (!this.fitted || !this.fitted.containsBox(box)) {
      const grow = this.workingBox();
      if (this.fitted) grow.union(this.fitted);
      this.fit(undefined, grow);
    } else {
      this.requestRender();
    }
  }

  /** (Re)build the faces, the textured overlays, the inner sides of open meshes and the
   * edges from the mesh, the materials and the style. */
  private buildFaces(): void {
    disposeObject(this.faces);
    disposeObject(this.backs);
    disposeObject(this.edges);
    this.faces = null;
    this.backs = null;
    this.edges = null;
    const mesh = this.meshData;
    const mode = faceMode(this.style);
    const lit = this.lit();
    this.resetHighlightState();
    if (mesh && mesh.indices.length > 0) {
      const g = new THREE.BufferGeometry();
      g.setAttribute("position", new THREE.BufferAttribute(mesh.positions, 3));
      g.setIndex(new THREE.BufferAttribute(mesh.indices, 1));
      if (mesh.uvs && mesh.uvs.length === (mesh.positions.length / 3) * 2) g.setAttribute("uv", new THREE.BufferAttribute(mesh.uvs.slice(), 2));
      // Lane 15: across smooth edges the kernel sends shared normals; otherwise flat.
      const smooth = !!mesh.normals && mesh.normals.length === mesh.positions.length && mesh.normals.length > 0;
      if (lit) {
        if (smooth) g.setAttribute("normal", new THREE.BufferAttribute(mesh.normals!.slice(), 3));
        else g.computeVertexNormals();
      }
      this.colourFaces(g);
      g.setAttribute("aPart", new THREE.BufferAttribute(partOfVertex(mesh, g.getAttribute("position").count), 1));
      g.computeBoundingSphere();
      g.computeBoundingBox();
      const common = {
        vertexColors: true,
        side: THREE.DoubleSide,
        polygonOffset: true,
        polygonOffsetFactor: 1,
        polygonOffsetUnits: 1,
        transparent: mode.xray,
        opacity: mode.xray ? 0.42 : 1,
        depthWrite: !mode.xray,
      };
      const mat = this.withPartState(lit ? new THREE.MeshLambertMaterial(common) : new THREE.MeshBasicMaterial(common));
      this.faces = new THREE.Mesh(g, mat);
      // Section cuts (lane 12): triangles grouped by the planes that clip them, each group
      // drawn with its own clipping material.
      const clip = this.triangleClipSets();
      if (clip) this.faces.material = this.clipGroups(g, mat, clip, mesh.indices.length / 3).map((m) => this.withPartState(m));
      this.faces.renderOrder = 2;
      this.faces.visible = mode.faces;
      this.faces.castShadow = lit;
      this.faces.receiveShadow = lit && !!this.shadows?.faces;
      this.scene.add(this.faces);
      // A solid only ever shows its outsides; an imported mesh may be open, so its inner sides
      // get their own tone (the default back-face colour), drawn as reversed triangles just in
      // front of the faces.
      const back: number[] = [];
      for (let t = 0; t < mesh.faceIds.length; t++) {
        if (mesh.parts[mesh.faceIds[t]]?.kind !== "mesh") continue;
        back.push(mesh.indices[t * 3], mesh.indices[t * 3 + 2], mesh.indices[t * 3 + 1]);
      }
      if (back.length && mode.faces && !mode.xray) {
        const bg = new THREE.BufferGeometry();
        const pos = g.getAttribute("position") as THREE.BufferAttribute;
        bg.setAttribute("position", pos);
        bg.setIndex(back);
        // The inner tone, shaded by which way each inner side faces, like the outer faces.
        const tone = new THREE.Color(BACK_FACE_COLOUR);
        const colors = new Float32Array(pos.count * 3);
        const [a, b, c, n] = [new THREE.Vector3(), new THREE.Vector3(), new THREE.Vector3(), new THREE.Vector3()];
        for (let t = 0; t < back.length; t += 3) {
          a.fromBufferAttribute(pos, back[t]);
          b.fromBufferAttribute(pos, back[t + 1]);
          c.fromBufferAttribute(pos, back[t + 2]);
          n.subVectors(b, a).cross(c.sub(a)).normalize();
          const k = n.z > 0.5 ? 1 : n.z < -0.5 ? 0.72 : Math.abs(n.y) >= Math.abs(n.x) ? 0.86 : 0.72;
          const col = tone.clone().multiplyScalar(k);
          for (const i of [back[t], back[t + 1], back[t + 2]]) col.toArray(colors, i * 3);
        }
        bg.setAttribute("color", new THREE.BufferAttribute(colors, 3));
        // No depth offset: the faces are pushed back by theirs, so these win where an inner
        // side faces the camera, and edges (drawn later) still show on top.
        this.backs = new THREE.Mesh(bg, new THREE.MeshBasicMaterial({ vertexColors: true, side: THREE.FrontSide }));
        if (clip) {
          const backSets = new Int32Array(back.length / 3);
          let k = 0;
          for (let t = 0; t < mesh.faceIds.length; t++) if (mesh.parts[mesh.faceIds[t]]?.kind === "mesh") backSets[k++] = clip.sets[t];
          this.backs.material = this.clipGroups(bg, this.backs.material as THREE.Material, { sets: backSets, planes: clip.planes }, back.length / 3);
        }
        this.backs.renderOrder = 2;
        this.scene.add(this.backs);
      }
    }
    this.buildOverlays();
    // Lane 15: soft and hidden edges apart (of the edges the section cuts leave).
    this.edgeSet = this.edgeSetNow();
    const hard = this.edgeSet?.hard;
    if (mesh && hard && hard.length > 0) {
      const eg = new THREE.BufferGeometry();
      eg.setAttribute("position", new THREE.BufferAttribute(hard, 3));
      eg.computeBoundingBox();
      // Wireframe and X-ray show the edges behind faces too.
      this.edges = new THREE.LineSegments(eg, new THREE.LineBasicMaterial({ color: this.ink(), depthTest: !mode.hiddenEdges, transparent: mode.hiddenEdges }));
      this.edges.renderOrder = 3;
      this.edges.visible = this.style.edges.edges;
      this.scene.add(this.edges);
    }
    this.styledEdgesKey = "";
    this.updateSun();
    // Fresh state textures: the highlight shown goes back in.
    this.applyHighlightState(this.highlight);
  }

  /** One mesh per texture over the faces that show it, drawn exactly over the faces (the
   * same positions and depth offset, LessEqual depth), with its own copy of their vertices. */
  private buildOverlays(): void {
    for (const c of [...this.textured.children]) disposeObject(c);
    this.texturedParts = 0;
    const mesh = this.meshData;
    const g = this.faces?.geometry;
    if (!mesh || !g || !this.faces?.visible || !g.getAttribute("uv")) return;
    const byDigest = new Map<string, { texture: THREE.Texture; tris: number[]; set: number }>();
    const counted = new Set<number>();
    const clip = this.triangleClipSets();
    for (let t = 0; t < mesh.faceIds.length; t++) {
      const part = mesh.faceIds[t];
      const tex = this.partTexture(part);
      if (!tex) continue;
      if (!counted.has(part)) {
        counted.add(part);
        this.texturedParts++;
      }
      const set = clip ? clip.sets[t] : -1;
      const key = `${tex.digest}|${set}`;
      let e = byDigest.get(key);
      if (!e) byDigest.set(key, (e = { texture: tex.texture, tris: [], set }));
      e.tris.push(mesh.indices[3 * t], mesh.indices[3 * t + 1], mesh.indices[3 * t + 2]);
    }
    const lit = this.lit();
    for (const { texture, tris, set } of byDigest.values()) {
      // Compact: only the vertices these triangles use.
      const remap = new Map<number, number>();
      const index: number[] = [];
      for (const v of tris) {
        let k = remap.get(v);
        if (k === undefined) remap.set(v, (k = remap.size));
        index.push(k);
      }
      const og = new THREE.BufferGeometry();
      for (const [name, size] of [["position", 3], ["uv", 2], ["color", 3], ["normal", 3], ["aPart", 1]] as const) {
        const at = g.getAttribute(name) as THREE.BufferAttribute | undefined;
        if (!at) continue;
        const src = at.array as Float32Array;
        const out = new Float32Array(remap.size * size);
        for (const [v, k] of remap) out.set(src.subarray(v * size, v * size + size), k * size);
        og.setAttribute(name, new THREE.BufferAttribute(out, size));
      }
      og.setIndex(index);
      const common = {
        map: texture,
        vertexColors: true,
        side: THREE.DoubleSide,
        polygonOffset: true,
        polygonOffsetFactor: 1,
        polygonOffsetUnits: 1,
        depthFunc: THREE.LessEqualDepth,
      };
      const om = new THREE.Mesh(og, this.withPartState(lit ? new THREE.MeshLambertMaterial(common) : new THREE.MeshBasicMaterial(common)));
      if (clip && set >= 0) (om.material as THREE.Material).clippingPlanes = clip.planes[set];
      om.renderOrder = 2.5;
      om.receiveShadow = this.faces.receiveShadow;
      om.castShadow = false;
      om.frustumCulled = false;
      this.textured.add(om);
    }
  }

  /** Place the sun and the shadow camera, or switch them off. */
  private updateSun(): void {
    const lit = this.lit();
    const s = this.shadows;
    this.renderer.shadowMap.enabled = lit;
    this.sunLight.visible = lit;
    this.ambient.visible = lit;
    this.groundShadow.visible = lit && !!s?.ground;
    if (!lit || !s) return;
    const dir = sunDirection(s);
    const up = dir[2] > 0;
    // Sunlight and shade: the sliders, 0..100.
    this.sunLight.intensity = up ? (s.light / 100) * 2.4 : 0;
    this.ambient.intensity = 0.35 + (s.dark / 100) * 1.6;
    this.sunLight.castShadow = up;
    const box = this.contentBox();
    const sphere = box.getBoundingSphere(new THREE.Sphere());
    const r = Math.max(sphere.radius, 0.05) * 1.6;
    const d = new THREE.Vector3(...dir);
    this.sunLight.position.copy(sphere.center).addScaledVector(d, r * 4);
    this.sunLight.target.position.copy(sphere.center);
    this.sunLight.target.updateMatrixWorld();
    const cam = this.sunLight.shadow.camera;
    cam.left = -r;
    cam.right = r;
    cam.top = r;
    cam.bottom = -r;
    cam.near = 0.001;
    cam.far = r * 8;
    cam.updateProjectionMatrix();
    this.sunLight.shadow.bias = -0.0004;
    this.sunLight.shadow.normalBias = r * 0.002;
    // The ground catches shadows a good way round the model.
    this.groundShadow.scale.setScalar(Math.max(r * 6, 1));
    this.groundShadow.position.set(sphere.center.x, sphere.center.y, GRID_DEPTH_OFFSET * 2);
    (this.groundShadow.material as THREE.ShadowMaterial).opacity = 0.18 + (1 - s.dark / 100) * 0.3;
  }

  /** Profile, extension and endpoint lines depend on the camera: rebuilt when it moves. */
  private updateStyledEdges(): void {
    const e = this.style.edges;
    const mesh = this.edgeData();
    const sets = this.edgeSet;
    const hard = sets?.hard ?? mesh?.edges;
    const showSoft = !!e.soft && !!sets && (sets.soft.length > 0 || sets.hidden.length > 0);
    // Soft edges turn into profiles where a smoothed surface turns away (lane 15).
    const softProfiles = !!sets && sets.soft.length > 0;
    const need = !!mesh && e.edges && (e.profiles || e.extensions || e.endpoints || showSoft || softProfiles);
    const cam = this.camera;
    const key = need ? `${cam.position.toArray().join()}|${(cam as THREE.OrthographicCamera).zoom}|${this.size.w}x${this.size.h}|${mesh?.edges.length}|${this.sectionKey}` : "";
    if (key === this.styledEdgesKey) return;
    this.styledEdgesKey = key;
    for (const c of [...this.styledEdges.children]) disposeObject(c);
    if (this.edges) {
      // Plain edges come back when the extensions are switched off.
      const pos = this.edges.geometry.getAttribute("position") as THREE.BufferAttribute;
      if (!e.extensions && hard && pos.array !== hard) this.edges.geometry.setAttribute("position", new THREE.BufferAttribute(hard, 3));
    }
    if (!need || !mesh) return;
    const mode = faceMode(this.style);
    const px = (p: THREE.Vector3) => this.metresPerPixelAt([p.x, p.y, p.z]);
    if (e.extensions && this.edges && hard) {
      this.edges.geometry.setAttribute("position", new THREE.BufferAttribute(extendedSegments(hard, (p) => 5 * px(p)), 3));
    }
    const pairs = (flat: number[]): Vec3[][] => {
      const out: Vec3[][] = [];
      for (let i = 0; i + 5 < flat.length; i += 6) out.push([[flat[i], flat[i + 1], flat[i + 2]], [flat[i + 3], flat[i + 4], flat[i + 5]]]);
      return out;
    };
    if (e.profiles) {
      const segs = profileSegments(hard ?? mesh.edges, sets ? sets.hardNormals : mesh.normals, this.camera);
      if (segs.length) {
        const l = this.fatLines(pairs(segs), this.ink(), 2.6, false, !mode.hiddenEdges);
        l.renderOrder = 3.5;
        this.styledEdges.add(l);
      }
    }
    // Lane 15: the silhouette of a smoothed surface, drawn as a plain line (heavier with
    // profiles on), so a softened cylinder still has its outline.
    if (softProfiles && sets) {
      const segs = profileSegments(sets.soft, sets.softNormals, this.camera);
      if (segs.length) {
        const l = this.fatLines(pairs(segs), this.ink(), e.profiles ? 2.6 : 1.2, false, !mode.hiddenEdges);
        l.renderOrder = 3.5;
        this.styledEdges.add(l);
      }
    }
    // Lane 15: "Soft and hidden edges" on: both drawn dashed and faint.
    if (showSoft && sets) {
      const flat = [...sets.soft, ...sets.hidden];
      if (flat.length) {
        const l = this.fatLines(pairs(flat), this.ink(), 1, true, !mode.hiddenEdges, 0.55);
        l.renderOrder = 3.4;
        this.styledEdges.add(l);
      }
    }
    if (e.endpoints && hard) {
      const segs = endpointSegments(hard, (p) => 7 * px(p));
      if (segs.length) {
        const l = this.fatLines(pairs(segs), this.ink(), 2.6, false, !mode.hiddenEdges);
        l.renderOrder = 3.5;
        this.styledEdges.add(l);
      }
    }
  }

  // ---- highlights and overlays -----------------------------------------------------------

  setHighlight(h: Highlight): void {
    const planes = (s: Highlight) => JSON.stringify(s.sections ?? null);
    const sectionsChanged = planes(h) !== planes(this.highlight);
    const before = this.highlight;
    this.highlight = h;
    // Lane 16b: what changed goes into the state textures (the faces are not recoloured and
    // the textured overlays not rebuilt: neither depends on the highlight any more).
    this.applyHighlightState(h);
    this.rebuildHighlights(before);
    if (sectionsChanged) this.rebuildSections();
    this.requestRender();
  }

  /** How many edge segments a highlight's whole solids have (once per list). */
  private ownerCounts = new WeakMap<readonly number[], number>();
  private ownerEdges(owners: readonly number[] | undefined): number {
    if (!owners?.length || !this.meshData) return 0;
    let n = this.ownerCounts.get(owners);
    if (n === undefined) this.ownerCounts.set(owners, (n = ownerEdgeCount(this.meshData, owners)));
    return n;
  }

  /** True when these solids' edges are drawn from the state texture, not as fat lines. */
  private manyOwners(owners: readonly number[] | undefined): boolean {
    return this.ownerEdges(owners) > FAT_HIGHLIGHT_EDGES;
  }

  /** New state textures for the mesh (all clear); the owner lines are made when needed. */
  private resetHighlightState(): void {
    this.partState?.dispose();
    this.ownerState?.dispose();
    disposeObject(this.ownerLines);
    this.ownerLines = null;
    this.ownerState = null;
    this.partState = null;
    this.applied = { selected: NONE, hover: NONE, faded: NONE, selOwners: NONE, hoverOwners: NONE };
    const mesh = this.meshData;
    if (!mesh) return;
    const max = this.renderer.capabilities.maxTextureSize;
    this.partState = new StateTexture(mesh.parts.length, max);
    this.stateUniforms.uPartState.value = this.partState.texture;
    this.stateUniforms.uPartW.value = this.partState.width;
  }

  /** Write `h` into the state textures: only the lists that changed, cleared then set. */
  private applyHighlightState(h: Highlight): void {
    const ps = this.partState;
    const swap = (t: StateTexture, key: keyof ViewportEngine["applied"], next: readonly number[], bit: number) => {
      const prev = this.applied[key];
      if (prev === next) return;
      t.mark(prev, bit, false);
      t.mark(next, bit, true);
      this.applied[key] = next;
    };
    if (ps) {
      swap(ps, "selected", h.selectedParts, SELECTED);
      swap(ps, "hover", h.hoverParts, HOVERED);
      swap(ps, "faded", h.fadedParts ?? NONE, FADED);
    }
    const selOwners = this.manyOwners(h.selectedEdgeOwners) ? h.selectedEdgeOwners! : NONE;
    const hoverOwners = this.manyOwners(h.hoverEdgeOwners) ? h.hoverEdgeOwners! : NONE;
    if (selOwners.length || hoverOwners.length) this.ensureOwnerLines();
    const os = this.ownerState;
    if (os) {
      swap(os, "selOwners", selOwners, SELECTED);
      swap(os, "hoverOwners", hoverOwners, HOVERED);
    }
    if (this.ownerLines) this.ownerLines.visible = selOwners.length + hoverOwners.length > 0;
  }

  /**
   * The faces' material, tinted by the part state texture: selected parts towards the
   * selection colour (0.45), hovered ones less (0.2), parts outside the open group towards
   * the background (0.6), applied to the vertex colour exactly as colourFaces used to bake it.
   */
  private withPartState<M extends THREE.Material>(m: M): M {
    m.onBeforeCompile = (shader) => {
      Object.assign(shader.uniforms, this.stateUniforms);
      shader.vertexShader = shader.vertexShader
        .replace(
          "#include <common>",
          `#include <common>
attribute float aPart;
uniform highp sampler2D uPartState;
uniform int uPartW;
uniform vec3 uSelTint;
uniform vec3 uFadeTo;
${STATE_FETCH}`,
        )
        .replace(
          "#include <color_vertex>",
          `#include <color_vertex>
#ifdef USE_COLOR
{
  int s = sawhorseState(uPartState, uPartW, int(aPart + 0.5));
  if ((s & ${SELECTED}) != 0) vColor.rgb = mix(vColor.rgb, uSelTint, 0.45);
  else if ((s & ${HOVERED}) != 0) vColor.rgb = mix(vColor.rgb, uSelTint, 0.2);
  if ((s & ${FADED}) != 0) vColor.rgb = mix(vColor.rgb, uFadeTo, 0.6);
}
#endif`,
        );
    };
    m.customProgramCacheKey = () => "sawhorse-part-state";
    return m;
  }

  /** Every edge of the mesh as one thin-line draw, each segment shown only while its solid's
   * byte in the owner state texture is set (in the selection colour, or the hover colour). */
  private ensureOwnerLines(): void {
    const mesh = this.meshData;
    if (this.ownerLines || !mesh) return;
    this.ownerState = new StateTexture(mesh.edgeOwners.length, this.renderer.capabilities.maxTextureSize);
    this.ownerUniforms.uOwnerState.value = this.ownerState.texture;
    this.ownerUniforms.uOwnerW.value = this.ownerState.width;
    const n = mesh.edgeOwner.length;
    const owner = new Float32Array(2 * n);
    for (let i = 0; i < n; i++) owner[2 * i] = owner[2 * i + 1] = mesh.edgeOwner[i];
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.BufferAttribute(mesh.edges, 3));
    g.setAttribute("aOwner", new THREE.BufferAttribute(owner, 1));
    // The renderer sorts by bounding sphere: the faces' (the edges lie on them), not a pass
    // over a million segments to compute one.
    g.boundingSphere = this.faces?.geometry.boundingSphere?.clone() ?? new THREE.Sphere(new THREE.Vector3(), 2e4);
    const m = new THREE.LineBasicMaterial({ color: 0xffffff });
    m.onBeforeCompile = (shader) => {
      Object.assign(shader.uniforms, this.ownerUniforms);
      shader.vertexShader = shader.vertexShader
        .replace(
          "#include <common>",
          `#include <common>
attribute float aOwner;
uniform highp sampler2D uOwnerState;
uniform int uOwnerW;
varying float vOwnerState;
${STATE_FETCH}`,
        )
        .replace(
          "#include <project_vertex>",
          `#include <project_vertex>
{
  int s = sawhorseState(uOwnerState, uOwnerW, int(aOwner + 0.5));
  vOwnerState = float(s);
  // Not highlighted: both ends outside the clip volume, so the segment is not drawn.
  if ((s & ${SELECTED | HOVERED}) == 0) gl_Position = vec4(2.0, 2.0, 2.0, 1.0);
}`,
        );
      shader.fragmentShader = shader.fragmentShader
        .replace(
          "#include <common>",
          `#include <common>
uniform vec3 uOwnerSel;
uniform vec3 uOwnerHover;
varying float vOwnerState;`,
        )
        .replace("vec4 diffuseColor = vec4( diffuse, opacity );", `vec4 diffuseColor = vec4( (int(vOwnerState + 0.5) & ${SELECTED}) != 0 ? uOwnerSel : uOwnerHover, opacity );`);
    };
    m.customProgramCacheKey = () => "sawhorse-owner-state";
    this.ownerLines = new THREE.LineSegments(g, m);
    this.ownerLines.renderOrder = 5;
    this.ownerLines.frustumCulled = false;
    this.ownerLines.visible = false;
    this.scene.add(this.ownerLines);
  }

  /** What the selection's highlight draws now (tests): fat segments (selected, hovered), the
   * solids whose edges come from the owner state texture, and the parts tinted as selected. */
  highlightCounts(): { fatSelected: number; fatHover: number; stateSolids: number; stateParts: number } {
    let fatSelected = 0;
    let fatHover = 0;
    for (const c of this.highlightGroup.children) {
      const n = c instanceof LineSegments2 ? ((c.geometry as LineSegmentsGeometry).getAttribute("instanceStart")?.count ?? 0) : 0;
      if (c.userData.role === "selected") fatSelected += n;
      else if (c.userData.role === "hover") fatHover += n;
    }
    let stateParts = 0;
    if (this.partState) for (const b of this.partState.data) if (b & SELECTED) stateParts++;
    return { fatSelected, fatHover, stateSolids: this.applied.selOwners.length, stateParts };
  }


  // ---- section planes (lane 12) ------------------------------------------------------

  /**
   * The section cuts to draw (`view`: caps, outlines and which planes clip what, from the
   * kernel) and the planes (from the scene graph). The style says whether cuts, planes and
   * the fill are shown.
   */
  setSections(view: SectionView | null, planes: GraphSection[]): void {
    const key = JSON.stringify([view, planes]);
    if (key === this.sectionKey) return;
    this.sectionKey = key;
    this.sectionView = view;
    this.sectionPlanes = planes;
    this.buildFaces();
    this.rebuildSections();
    this.requestRender();
  }

  /** The drawn plane under canvas pixel (x, y): its id, ray parameter, and whether the hit
   * is its reverse control (the arrow at its centre). */
  pickSection(x: number, y: number): { section: number; t: number; control: boolean } | null {
    return pickSection(this.ray(x, y), this.sectionPlanes, this.style.section_planes, (p) => this.metresPerPixelAt(p));
  }

  /** What the viewport shows of the sections (tests and captures). */
  sectionsState(): { planes: number; active: number; clipped: number; caps: number; outline: number; fill: boolean } {
    const v = this.sectionView;
    return {
      planes: this.sectionPlanes.filter((s) => s.visible).length,
      active: this.sectionPlanes.filter((s) => s.active).length,
      clipped: (v?.clips ?? []).reduce((n, c) => n + c.objects.length, 0),
      caps: (v?.cuts ?? []).reduce((n, c) => n + c.indices.length / 3, 0),
      outline: (v?.cuts ?? []).reduce((n, c) => n + c.outline.length / 6, 0),
      fill: this.style.section_fill,
    };
  }

  /** Which clip set each triangle of the view mesh is in (-1: none), and the sets' planes. */
  private triangleClipSets(): { sets: Int32Array; planes: THREE.Plane[][] } | null {
    const mesh = this.meshData;
    const v = this.sectionView;
    if (!mesh || !v?.clips.length || !this.style.section_cuts) return null;
    const perPart = partClipPlanes(mesh, v);
    const index = new Map<unknown, number>();
    const planes: THREE.Plane[][] = [];
    for (const ps of perPart) {
      if (!ps || index.has(ps)) continue;
      index.set(ps, planes.length);
      planes.push(ps.map(([x, y, z, c]) => new THREE.Plane(new THREE.Vector3(x, y, z), c)));
    }
    if (!planes.length) return null;
    const sets = new Int32Array(mesh.faceIds.length);
    for (let t = 0; t < mesh.faceIds.length; t++) {
      const ps = perPart[mesh.faceIds[t]];
      sets[t] = ps ? (index.get(ps) ?? -1) : -1;
    }
    return { sets, planes };
  }

  /** Geometry groups over runs of triangles in the same clip set, and the materials for
   * them: `base` unclipped first, then a copy per set clipped by its planes. */
  private clipGroups(g: THREE.BufferGeometry, base: THREE.Material, clip: { sets: Int32Array; planes: THREE.Plane[][] }, triangles: number): THREE.Material[] {
    g.clearGroups();
    let start = 0;
    for (let t = 1; t <= triangles; t++) {
      if (t === triangles || clip.sets[t] !== clip.sets[start]) {
        g.addGroup(start * 3, (t - start) * 3, clip.sets[start] + 1);
        start = t;
      }
    }
    return [
      base,
      ...clip.planes.map((ps) => {
        const m = base.clone();
        m.clippingPlanes = ps;
        m.clipShadows = true;
        return m;
      }),
    ];
  }

  /** The edges as drawn: the view mesh's, less what active planes cut away. */
  private edgeData(): { edges: Float32Array; normals: Float32Array | undefined; flags: Uint8Array | undefined } | null {
    const mesh = this.meshData;
    if (!mesh) return null;
    const c = this.style.section_cuts ? clippedEdges(mesh, this.sectionView) : { edges: mesh.edges, normals: mesh.edgeNormals, flags: mesh.edgeFlags };
    return { edges: c.edges, normals: c.normals, flags: c.flags };
  }

  /** The drawn edges by how they are drawn (lane 15), of those the section cuts leave. */
  private edgeSetNow(): EdgeSets | null {
    const d = this.edgeData();
    return d ? edgeSets(d.edges, d.normals, d.flags) : null;
  }

  /** The planes (rectangles, the arrows at their corners and the reverse control at their
   * centre), and each active cut's fill and outline. */
  private rebuildSections(): void {
    for (const c of [...this.sectionGroup.children]) disposeObject(c);
    const st = this.style;
    const h = this.highlight.sections;
    const selected = new Set(h?.selected ?? []);
    if (st.section_planes) {
      for (const s of this.sectionPlanes) {
        if (!s.visible) continue;
        const colour = selected.has(s.id) ? this.theme.sel : h?.hover === s.id ? this.theme.hover : s.active ? this.theme.section : "#8d9189";
        const c = sectionCorners(s);
        const rim = this.fatLines(
          c.map((p, i) => [p, c[(i + 1) % 4]]),
          colour,
          selected.has(s.id) ? 3 : s.active ? 2 : 1.5,
          !s.active,
          true,
          s.active || selected.has(s.id) ? 1 : 0.8,
        );
        rim.renderOrder = 7;
        this.sectionGroup.add(rim);
        const arrows = sectionArrows(s);
        const pairs = (flat: Vec3[]) => {
          const out: Vec3[][] = [];
          for (let i = 0; i + 1 < flat.length; i += 2) out.push([flat[i], flat[i + 1]]);
          return out;
        };
        const ar = this.fatLines(pairs([...arrows.corners, ...arrows.centre]), colour, 1.75, false, true, 0.9);
        ar.renderOrder = 7;
        this.sectionGroup.add(ar);
        const fg = new THREE.BufferGeometry();
        fg.setAttribute("position", new THREE.Float32BufferAttribute(c.flat(), 3));
        fg.setIndex([0, 1, 2, 0, 2, 3]);
        const fill = new THREE.Mesh(fg, new THREE.MeshBasicMaterial({ color: new THREE.Color(this.cssColor(colour)), transparent: true, opacity: selected.has(s.id) ? 0.14 : 0.07, side: THREE.DoubleSide, depthWrite: false }));
        fill.renderOrder = 6;
        this.sectionGroup.add(fill);
      }
    }
    const v = this.sectionView;
    if (v && st.section_cuts) {
      const mode = faceMode(st);
      for (const cut of v.cuts) {
        if (st.section_fill && mode.faces && cut.indices.length) {
          const g = new THREE.BufferGeometry();
          g.setAttribute("position", new THREE.Float32BufferAttribute(cut.positions, 3));
          g.setIndex(cut.indices);
          const colour = st.section_fill_color ? rgbCss(st.section_fill_color) : this.theme.sectionFill;
          const cap = new THREE.Mesh(
            g,
            new THREE.MeshBasicMaterial({ color: new THREE.Color(colour), side: THREE.DoubleSide, polygonOffset: true, polygonOffsetFactor: -1, polygonOffsetUnits: -1, transparent: mode.xray, opacity: mode.xray ? 0.5 : 1 }),
          );
          cap.renderOrder = 2.7;
          this.sectionGroup.add(cap);
        }
        if (cut.outline.length) {
          const pairs: Vec3[][] = [];
          const o = cut.outline;
          for (let i = 0; i + 5 < o.length; i += 6) pairs.push([[o[i], o[i + 1], o[i + 2]], [o[i + 3], o[i + 4], o[i + 5]]]);
          const colour = st.section_line_color ? rgbCss(st.section_line_color) : this.ink();
          const line = this.fatLines(pairs, colour, st.section_line_width, false, true);
          line.renderOrder = 3.6;
          this.sectionGroup.add(line);
        }
      }
    }
  }

  private fatLines(pairs: Vec3[][], color: string, width: number, dashed = false, depthTest = true, opacity = 1): LineSegments2 {
    return this.fatLinesFlat(flatPairs(pairs), color, width, dashed, depthTest, opacity);
  }

  /** Fat lines from xyz pairs, six floats a segment (lane 16b: no array of arrays). */
  private fatLinesFlat(flat: Float32Array, color: string, width: number, dashed = false, depthTest = true, opacity = 1): LineSegments2 {
    const g = new LineSegmentsGeometry();
    g.setPositions(flat);
    const mat = new LineMaterial({
      color: new THREE.Color(this.cssColor(color)).getHex(),
      linewidth: width,
      dashed,
      dashSize: 6,
      gapSize: 4,
      depthTest,
      transparent: true,
      opacity,
      worldUnits: false,
    });
    mat.resolution.set(this.size.w, this.size.h);
    if (dashed) {
      // Dash lengths in pixels: scale by the metres per pixel at the target.
      const mpp = this.pixelAtTarget();
      mat.dashSize = 6 * mpp;
      mat.gapSize = 4 * mpp;
    }
    const l = new LineSegments2(g, mat);
    if (dashed) l.computeLineDistances();
    l.frustumCulled = false;
    return l;
  }

  /**
   * The fat highlight lines: explicit edges, and whole solids' edges while they hold at most
   * FAT_HIGHLIGHT_EDGES segments (more are drawn from the owner state texture). The hover
   * and the selection are rebuilt apart, each only when its own lists changed (the pointer
   * moves often; the selection does not). No `before`: rebuild both (theme, resize).
   */
  private rebuildHighlights(before?: Highlight): void {
    const h = this.highlight;
    const mesh = this.meshData;
    const segs = (pairs: [Vec3, Vec3][], owners: number[] | undefined) => {
      const flat = flatPairs(pairs);
      return mesh && owners?.length && !this.manyOwners(owners) ? ownerSegments(mesh, owners, flat) : flat;
    };
    const drop = (role: string) => {
      for (const c of [...this.highlightGroup.children]) if (c.userData.role === role) disposeObject(c);
    };
    if (!before || before.hoverEdges !== h.hoverEdges || before.hoverEdgeOwners !== h.hoverEdgeOwners) {
      drop("hover");
      const flat = segs(h.hoverEdges, h.hoverEdgeOwners);
      if (flat.length) {
        const l = this.fatLinesFlat(flat, this.theme.hover, 2.5, false, true, 0.55);
        l.renderOrder = 4;
        l.userData.role = "hover";
        this.highlightGroup.add(l);
      }
    }
    if (!before || before.selectedEdges !== h.selectedEdges || before.selectedEdgeOwners !== h.selectedEdgeOwners || before.selectedPoints !== h.selectedPoints) {
      drop("selected");
      const flat = segs(h.selectedEdges, h.selectedEdgeOwners);
      if (flat.length) {
        const l = this.fatLinesFlat(flat, this.theme.sel, 3, false, true);
        l.renderOrder = 5;
        l.userData.role = "selected";
        this.highlightGroup.add(l);
      }
      if (h.selectedPoints.length) {
        const g = new THREE.BufferGeometry();
        g.setAttribute("position", new THREE.Float32BufferAttribute(h.selectedPoints.flat(), 3));
        const pts = new THREE.Points(g, new THREE.PointsMaterial({ color: this.theme.sel, size: 8, sizeAttenuation: false, depthTest: false }));
        pts.renderOrder = 6;
        pts.userData.role = "selected";
        this.highlightGroup.add(pts);
      }
    }
  }

  /** Replace the tool overlays (previews, inference lines). */
  setOverlay(items: OverlayItem[]): void {
    this.overlays = items;
    this.rebuildOverlays();
    this.requestRender();
  }

  private rebuildOverlays(): void {
    for (const c of [...this.overlayGroup.children]) disposeObject(c);
    for (const it of this.overlays) {
      const pairs: Vec3[][] = [];
      const p = it.points;
      if (it.kind === "segments") {
        for (let i = 0; i + 1 < p.length; i += 2) pairs.push([p[i], p[i + 1]]);
      } else {
        for (let i = 0; i + 1 < p.length; i++) pairs.push([p[i], p[i + 1]]);
        if (it.kind === "loop" && p.length > 2) pairs.push([p[p.length - 1], p[0]]);
      }
      if (!pairs.length) continue;
      const l = this.fatLines(pairs, it.color, it.width ?? 2, !!it.dashed, !!it.depthTest);
      l.renderOrder = 10;
      this.overlayGroup.add(l);
    }
  }

  // ---- camera ----------------------------------------------------------------------------

  /** The eye ray through canvas CSS pixel (x, y). */
  ray(x: number, y: number): { origin: Vec3; dir: Vec3 } {
    this.raycaster.setFromCamera(this.ndc(x, y), this.camera);
    const { origin, direction } = this.raycaster.ray;
    return { origin: [origin.x, origin.y, origin.z], dir: [direction.x, direction.y, direction.z] };
  }

  /** How big a CSS pixel is in the world, for pixel tolerances (inference, grid). */
  pixelScale(): PixelScale {
    const cam = this.camera;
    const fwd = new THREE.Vector3();
    cam.getWorldDirection(fwd);
    const eye: Vec3 = [cam.position.x, cam.position.y, cam.position.z];
    if (cam instanceof THREE.PerspectiveCamera) {
      const scale = (2 * Math.tan((cam.fov * Math.PI) / 360)) / (this.size.h * cam.zoom);
      return { ortho: false, scale, eye, forward: [fwd.x, fwd.y, fwd.z] };
    }
    const scale = (cam.top - cam.bottom) / cam.zoom / this.size.h;
    return { ortho: true, scale, eye, forward: [fwd.x, fwd.y, fwd.z] };
  }

  metresPerPixelAt(p: Vec3): number {
    const px = this.pixelScale();
    if (px.ortho) return px.scale;
    const d = (p[0] - px.eye[0]) * px.forward[0] + (p[1] - px.eye[1]) * px.forward[1] + (p[2] - px.eye[2]) * px.forward[2];
    return px.scale * Math.max(d, 1e-6);
  }

  /** Canvas CSS pixel of a world point. */
  worldToScreen(p: Vec3): [number, number] {
    return this.toScreen(new THREE.Vector3(p[0], p[1], p[2]));
  }

  /** The default working area: about 2 ft around the origin. */
  static readonly WORKING_AREA = new THREE.Box3(new THREE.Vector3(-0.15, -0.15, 0), new THREE.Vector3(0.6, 0.45, 0.3));

  /** The model plus the default working area (what standard views frame). */
  workingBox(): THREE.Box3 {
    return this.contentBox().union(ViewportEngine.WORKING_AREA);
  }

  /** Bounding box of the model, or the default working area when it is empty (Zoom Extents). */
  contentBox(): THREE.Box3 {
    const box = new THREE.Box3();
    if (this.faces?.geometry.boundingBox) box.union(this.faces.geometry.boundingBox);
    if (this.edges?.geometry.boundingBox) box.union(this.edges.geometry.boundingBox);
    if (box.isEmpty()) return ViewportEngine.WORKING_AREA.clone();
    return box;
  }

  /** The last standard view, or null once the user has orbited away from it. */
  currentView(): ViewId | null {
    return this.view;
  }

  currentProjection(): Projection {
    return this.projection;
  }

  /** A standard view: its direction, its projection (Persp is the perspective one), framed. */
  setView(view: ViewId): void {
    this.view = view;
    this.useProjection(projectionForView(view));
    this.fit(view, this.workingBox());
  }

  /** Switch parallel / perspective keeping the target, direction and on-screen size. */
  setProjection(p: Projection): void {
    if (p === this.projection) return;
    const target = this.controls.target.clone();
    const eyeDir = this.camera.position.clone().sub(target);
    const dist = eyeDir.length();
    eyeDir.normalize();
    const up = this.camera.up.clone();
    if (p === "perspective") {
      const h = this.frame / this.ortho.zoom;
      const d = equivalentPerspectiveDistance(h);
      this.useProjection(p);
      this.camera.up.copy(up);
      this.camera.position.copy(target).addScaledVector(eyeDir, d);
      this.persp.near = Math.max(d / 1000, 0.001);
      this.persp.far = d * 100;
    } else {
      this.useProjection(p);
      this.frame = equivalentOrthoHalfHeight(dist);
      this.ortho.zoom = 1;
      this.camera.up.copy(up);
      const r = Math.max(this.contentBox().getBoundingSphere(new THREE.Sphere()).radius, 0.05);
      this.camera.position.copy(target).addScaledVector(eyeDir, Math.max(dist, r * 20));
      this.ortho.near = -r * 100;
      this.ortho.far = r * 200 + dist;
    }
    this.controls.target.copy(target);
    this.updateProjection();
    this.controls.update();
    this.updateGridVisibility();
    this.requestRender();
  }

  toggleProjection(): void {
    this.setProjection(this.projection === "parallel" ? "perspective" : "parallel");
  }

  private useProjection(p: Projection): void {
    this.projection = p;
    this.camera = p === "perspective" ? this.persp : this.ortho;
    this.controls.object = this.camera;
  }

  /** Zoom extents: frame `box` (default: the model), keeping the current direction (or a view's). */
  fit(view?: ViewId, frame?: THREE.Box3): void {
    const box = frame ?? this.contentBox();
    this.fitted = box.clone().expandByScalar(box.getSize(new THREE.Vector3()).length() * 0.15);
    const sphere = box.getBoundingSphere(new THREE.Sphere());
    let dir: THREE.Vector3;
    let up: THREE.Vector3;
    if (view) {
      dir = new THREE.Vector3(...VIEW_DIRS[view].dir);
      up = new THREE.Vector3(...VIEW_DIRS[view].up);
    } else {
      dir = this.camera.position.clone().sub(this.controls.target).normalize();
      up = this.camera.up.clone();
    }
    const r = Math.max(sphere.radius, 0.05);
    const target = sphere.center;
    this.camera.up.copy(up);
    if (this.camera instanceof THREE.PerspectiveCamera) {
      const dist = (r * 1.25) / Math.sin(THREE.MathUtils.degToRad(this.camera.fov / 2));
      this.camera.zoom = 1;
      this.camera.position.copy(target).addScaledVector(dir, dist);
      this.camera.near = Math.max(dist / 1000, 0.001);
      this.camera.far = dist * 100;
    } else {
      this.frame = r * 1.2;
      this.camera.zoom = 1;
      this.camera.position.copy(target).addScaledVector(dir, r * 20);
      this.camera.near = -r * 100;
      this.camera.far = r * 200;
    }
    this.controls.target.copy(target);
    this.updateProjection();
    this.controls.update();
    this.updateGridVisibility();
    this.requestRender();
  }

  private updateProjection(): void {
    const aspect = this.size.w / Math.max(this.size.h, 1);
    if (this.camera instanceof THREE.PerspectiveCamera) {
      this.camera.aspect = aspect;
    } else {
      this.camera.top = this.frame;
      this.camera.bottom = -this.frame;
      this.camera.left = -this.frame * aspect;
      this.camera.right = this.frame * aspect;
    }
    this.camera.updateProjectionMatrix();
  }

  /** The camera as an SVG view drawing needs it (crates/export svg::ViewSpec). Metres. */
  viewSpec(): { eye: Vec3; target: Vec3; up: Vec3; ortho: boolean; fov: number; half_height: number; width: number; height: number } {
    const cam = this.camera;
    const p = cam.position;
    const t = this.controls.target;
    const u = cam.up;
    const base = { eye: [p.x, p.y, p.z] as Vec3, target: [t.x, t.y, t.z] as Vec3, up: [u.x, u.y, u.z] as Vec3, width: this.size.w, height: this.size.h };
    if (cam instanceof THREE.PerspectiveCamera) {
      // Zoom narrows the field of view.
      const fov = (2 * Math.atan(Math.tan(THREE.MathUtils.degToRad(cam.fov / 2)) / cam.zoom) * 180) / Math.PI;
      return { ...base, ortho: false, fov, half_height: 0 };
    }
    return { ...base, ortho: true, fov: PERSPECTIVE_FOV_DEG, half_height: (cam.top - cam.bottom) / 2 / cam.zoom };
  }

  /** Look through a saved scene's camera (metres, Z up). */
  lookAt(eye: Vec3, target: Vec3, up: Vec3, projection: Projection, fovDeg = PERSPECTIVE_FOV_DEG): void {
    this.view = null;
    this.useProjection(projection);
    const e = new THREE.Vector3(...eye);
    const t = new THREE.Vector3(...target);
    const dist = Math.max(e.distanceTo(t), 1e-3);
    const r = Math.max(this.contentBox().getBoundingSphere(new THREE.Sphere()).radius, 0.05);
    this.camera.up.set(...up).normalize();
    if (this.camera instanceof THREE.PerspectiveCamera) {
      this.camera.fov = fovDeg;
      this.camera.zoom = 1;
      this.camera.position.copy(e);
      this.camera.near = Math.max(dist / 1000, 0.001);
      this.camera.far = Math.max(dist, r) * 100;
    } else {
      // What a perspective camera at this distance would show at the target.
      this.frame = dist * Math.tan(THREE.MathUtils.degToRad(fovDeg / 2));
      this.camera.zoom = 1;
      const dir = e.clone().sub(t).normalize();
      this.camera.position.copy(t).addScaledVector(dir, Math.max(dist, r * 20));
      this.camera.near = -r * 100;
      this.camera.far = r * 200 + dist;
    }
    this.controls.target.copy(t);
    this.fitted = this.contentBox();
    this.updateProjection();
    this.controls.update();
    this.updateGridVisibility();
    this.requestRender();
  }

  /** A picture of the current view for a .shz thumbnail (lane 15): WebP (JPEG where the
   * browser cannot write WebP), at most `max` pixels on its long side and at most `limit`
   * bytes, stepping the quality and then the size down until it fits; empty if it cannot. */
  async snapshotThumbnail(max = 512, limit = 100 * 1024): Promise<Uint8Array> {
    this.beforeRender();
    this.renderer.render(this.scene, this.camera);
    const src = this.renderer.domElement;
    for (const side of [max, Math.round(max * 0.75), Math.round(max / 2), Math.round(max / 4)]) {
      const k = Math.min(1, side / Math.max(src.width, src.height));
      const c = document.createElement("canvas");
      c.width = Math.max(1, Math.round(src.width * k));
      c.height = Math.max(1, Math.round(src.height * k));
      const g = c.getContext("2d");
      if (!g) return new Uint8Array();
      // JPEG has no transparency: paint the viewport's background first.
      g.fillStyle = this.style.background ? rgbCss(this.style.background) : this.theme.vp;
      g.fillRect(0, 0, c.width, c.height);
      g.drawImage(src, 0, 0, c.width, c.height);
      for (const quality of [0.82, 0.7, 0.55, 0.4]) {
        for (const type of ["image/webp", "image/jpeg"]) {
          const blob = await new Promise<Blob | null>((resolve) => c.toBlob(resolve, type, quality));
          // A browser that cannot write WebP hands back a PNG: try JPEG instead.
          if (!blob || blob.type !== type) continue;
          if (blob.size <= limit) return new Uint8Array(await blob.arrayBuffer());
          break;
        }
      }
    }
    return new Uint8Array();
  }

  /** A compact description of the camera (tests and captures watch it change). */
  cameraSummary(): { eye: Vec3; target: Vec3; projection: Projection; view: ViewId | null; zoom: number } {
    const r = (v: number) => Math.round(v * 1e4) / 1e4;
    const p = this.camera.position;
    const t = this.controls.target;
    return { eye: [r(p.x), r(p.y), r(p.z)], target: [r(t.x), r(t.y), r(t.z)], projection: this.projection, view: this.view, zoom: r(this.camera.zoom) };
  }

  resize(w: number, h: number): void {
    this.size = { w: Math.max(1, w), h: Math.max(1, h) };
    this.renderer.setSize(this.size.w, this.size.h, false);
    this.updateProjection();
    this.rebuildHighlights();
    this.rebuildOverlays();
    this.updateGridVisibility();
    this.requestRender();
  }

  /** Called after every frame that moved the camera or changed the scene. */
  onRender(cb: () => void): () => void {
    this.listeners.add(cb);
    return () => this.listeners.delete(cb);
  }

  requestRender(): void {
    if (this.queued || this.disposed) return;
    this.queued = true;
    requestAnimationFrame(() => {
      this.queued = false;
      if (this.disposed) return;
      this.beforeRender();
      this.renderer.render(this.scene, this.camera);
      for (const cb of this.listeners) cb();
    });
  }

  /** Average milliseconds per frame over `n` full renders, turning the camera a little each
   * time (so profiles and shadows are redone): the 60 fps budget check. */
  benchmark(n = 120): number {
    const gl = this.renderer.getContext();
    const target = this.controls.target.clone();
    const start = this.camera.position.clone();
    const t0 = performance.now();
    for (let i = 0; i < n; i++) {
      const a = (i / n) * Math.PI * 0.5;
      const rel = start.clone().sub(target).applyAxisAngle(new THREE.Vector3(0, 0, 1), a);
      this.camera.position.copy(target).add(rel);
      this.camera.lookAt(target);
      this.beforeRender();
      this.renderer.render(this.scene, this.camera);
    }
    gl.finish();
    const ms = (performance.now() - t0) / n;
    this.camera.position.copy(start);
    this.camera.lookAt(target);
    this.requestRender();
    return ms;
  }

  /** What follows the camera: the backdrop's horizon and the profile / extension lines. */
  private beforeRender(): void {
    setBackdrop(this.backdrop, this.style, this.theme.vp, this.camera, this.controls.target);
    this.renderer.setClearColor(new THREE.Color(this.style.background ? rgbCss(this.style.background) : this.theme.vp), 1);
    this.updateStyledEdges();
  }

  /** Project a world point to canvas CSS pixels. */
  private toScreen(v: THREE.Vector3): [number, number] {
    const p = v.clone().project(this.camera);
    return [((p.x + 1) / 2) * this.size.w, ((1 - p.y) / 2) * this.size.h];
  }

  /** The rendered colour at canvas CSS pixel (x, y) (the drawing buffer is preserved). */
  readPixel(x: number, y: number): [number, number, number] {
    const gl = this.renderer.getContext();
    const ratio = this.renderer.getPixelRatio();
    const px = new Uint8Array(4);
    gl.readPixels(Math.round(x * ratio), Math.round((this.size.h - y) * ratio), 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    return [px[0], px[1], px[2]];
  }

  /** Canvas size in CSS pixels. */
  viewportSize(): { w: number; h: number } {
    return { ...this.size };
  }

  /** What the render passes (src/render/passes.ts) draw from: this renderer, the current camera and the model. */
  captureSource(): { renderer: THREE.WebGLRenderer; camera: THREE.Camera; mesh: MeshData | null; size: { w: number; h: number }; materials: MaterialInfo[] } {
    return { renderer: this.renderer, camera: this.camera, mesh: this.meshData, size: { ...this.size }, materials: [...this.materials.values()] };
  }

  private ndc(x: number, y: number): THREE.Vector2 {
    return new THREE.Vector2((x / this.size.w) * 2 - 1, -(y / this.size.h) * 2 + 1);
  }

  /** Canvas CSS pixel over a visible point of part `part` (its largest triangle's centroid). */
  partScreenCenter(part: number): [number, number] | null {
    const m = this.meshData;
    if (!m) return null;
    let best = -1;
    let bestArea = 0;
    const a = new THREE.Vector3();
    const b = new THREE.Vector3();
    const c = new THREE.Vector3();
    for (let t = 0; t < m.faceIds.length; t++) {
      if (m.faceIds[t] !== part) continue;
      a.fromArray(m.positions, m.indices[3 * t] * 3);
      b.fromArray(m.positions, m.indices[3 * t + 1] * 3);
      c.fromArray(m.positions, m.indices[3 * t + 2] * 3);
      const area = b.clone().sub(a).cross(c.clone().sub(a)).length();
      if (area > bestArea) {
        bestArea = area;
        best = t;
      }
    }
    if (best < 0) return null;
    a.fromArray(m.positions, m.indices[3 * best] * 3);
    b.fromArray(m.positions, m.indices[3 * best + 1] * 3);
    c.fromArray(m.positions, m.indices[3 * best + 2] * 3);
    return this.toScreen(a.add(b).add(c).multiplyScalar(1 / 3));
  }

  /**
   * Coarse three.js pick: the nearest mesh part under canvas pixel (x, y), else the ground
   * plane (part -1). Tools resolve clicks through the inference engine instead; this stays
   * for the selection box and tests.
   */
  pick(x: number, y: number): { point: Vec3; part: number } | null {
    this.raycaster.setFromCamera(this.ndc(x, y), this.camera);
    if (this.faces && this.meshData) {
      const hit = this.raycaster.intersectObject(this.faces, false)[0];
      if (hit && hit.faceIndex != null) {
        const part = this.meshData.faceIds[hit.faceIndex] ?? -1;
        return { point: [hit.point.x, hit.point.y, hit.point.z], part };
      }
    }
    const ground = new THREE.Plane(new THREE.Vector3(0, 0, 1), 0);
    const p = this.raycaster.ray.intersectPlane(ground, new THREE.Vector3());
    return p ? { point: [p.x, p.y, p.z], part: -1 } : null;
  }

  /** Is world point `p` hidden behind a face nearer the eye? (screen-space helpers) */
  occluded(p: Vec3): boolean {
    if (!this.faces) return false;
    const v = new THREE.Vector3(...p);
    const [sx, sy] = this.toScreen(v);
    this.raycaster.setFromCamera(this.ndc(sx, sy), this.camera);
    const hit = this.raycaster.intersectObject(this.faces, false)[0];
    if (!hit) return false;
    const along = v.clone().sub(this.raycaster.ray.origin).dot(this.raycaster.ray.direction);
    const eps = Math.max(1e-4, (this.faces.geometry.boundingSphere?.radius ?? 1) * 1e-3);
    return hit.distance < along - eps;
  }

  /** Edge segments by how they are drawn (lane 15), and whether the faces carry smooth
   * normals: a test and capture hook. */
  edgeCounts(): { hard: number; soft: number; hidden: number; smoothNormals: boolean } {
    const m = this.meshData;
    const s = this.edgeSet;
    return {
      hard: (s?.hard.length ?? m?.edges.length ?? 0) / 6,
      soft: (s?.soft.length ?? 0) / 6,
      hidden: (s?.hidden.length ?? 0) / 6,
      smoothNormals: !!m?.normals && m.normals.length > 0,
    };
  }

  /** Screen positions (canvas CSS px) of visible edge endpoints: a test and capture hook. */
  visibleEndpoints(): [number, number][] {
    const out: [number, number][] = [];
    const e = this.meshData?.edges;
    if (!e) return out;
    for (let i = 0; i + 2 < e.length; i += 3) {
      const p: Vec3 = [e[i], e[i + 1], e[i + 2]];
      if (!this.occluded(p)) out.push(this.worldToScreen(p));
    }
    return out;
  }

  dispose(): void {
    this.disposed = true;
    this.setOverlay([]);
    this.setMesh(null);
    this.listeners.clear();
    this.textures.dispose();
    disposeObject(this.sectionGroup);
    disposeObject(this.backdrop);
    disposeObject(this.groundShadow);
    this.controls.dispose();
    disposeObject(this.gridMinor);
    disposeObject(this.gridMajor);
    disposeObject(this.axes);
    this.renderer.dispose();
  }
}
