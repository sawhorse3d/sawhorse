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
import type { MeshData } from "../kernel/types";
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

interface Theme {
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
}

export const NO_HIGHLIGHT: Highlight = { hoverParts: [], selectedParts: [], hoverEdges: [], selectedEdges: [], selectedPoints: [] };

function readTheme(el: Element): Theme {
  const cs = getComputedStyle(el);
  const v = (n: string, fallback: string) => cs.getPropertyValue(n).trim() || fallback;
  return {
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

  constructor(
    canvas: HTMLCanvasElement,
    private themeRoot: Element = document.documentElement,
  ) {
    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, preserveDrawingBuffer: true });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.theme = readTheme(themeRoot);
    this.scene.add(this.axes, this.highlightGroup, this.overlayGroup);

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
    this.gridMinor = mk(gridLines(this.spec.minor, this.gridHalf, this.spec.ratio), this.theme.grid, -2);
    this.gridMajor = mk(gridLines(this.spec.major, this.gridHalf), this.theme.gridMajor, -1);
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
    if (this.gridMinor) (this.gridMinor.material as THREE.LineBasicMaterial).color.set(this.theme.grid);
    if (this.gridMajor) (this.gridMajor.material as THREE.LineBasicMaterial).color.set(this.theme.gridMajor);
    if (this.edges) (this.edges.material as THREE.LineBasicMaterial).color.set(this.theme.edge);
    if (this.faces) this.colourFaces(this.faces.geometry);
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

  /** Colour each triangle by its normal with the mock's three face tokens (top / front / side). */
  private colourFaces(geom: THREE.BufferGeometry): void {
    const pos = geom.getAttribute("position") as THREE.BufferAttribute;
    const index = geom.getIndex();
    if (!index) return;
    const colors = new Float32Array(pos.count * 3);
    const top = new THREE.Color(this.theme.faceTop);
    const front = new THREE.Color(this.theme.face);
    const side = new THREE.Color(this.theme.faceSide);
    const sel = new THREE.Color(this.theme.sel);
    const warn = new THREE.Color("#d9a33a");
    const tint = (c: THREE.Color, k: number) => c.clone().lerp(sel, k);
    const selected = new Set(this.highlight.selectedParts);
    const hovered = new Set(this.highlight.hoverParts);
    const ids = this.meshData?.faceIds;
    const a = new THREE.Vector3();
    const b = new THREE.Vector3();
    const c = new THREE.Vector3();
    const n = new THREE.Vector3();
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
      let col = [top, front, side][kind];
      const info = part >= 0 ? this.meshData?.parts[part] : undefined;
      if (info?.color) {
        // A material colour, shaded like the theme's three face tones.
        const [r, g, b] = info.color;
        col = new THREE.Color().setRGB(r / 255, g / 255, b / 255, THREE.SRGBColorSpace).multiplyScalar([1, 0.86, 0.72][kind]);
      }
      // Imported geometry that is not a solid reads amber-tinted: it cannot be pushed or printed.
      if (info?.kind === "mesh") col = col.clone().lerp(warn, 0.3);
      if (selected.has(part)) col = tint(col, 0.45);
      else if (hovered.has(part)) col = tint(col, 0.2);
      for (const i of [ia, ib, ic]) col.toArray(colors, i * 3);
    }
    geom.setAttribute("color", new THREE.BufferAttribute(colors, 3));
  }

  setMesh(mesh: MeshData | null): void {
    disposeObject(this.faces);
    disposeObject(this.edges);
    this.faces = null;
    this.edges = null;
    this.meshData = mesh;
    if (mesh && mesh.indices.length > 0) {
      const g = new THREE.BufferGeometry();
      g.setAttribute("position", new THREE.BufferAttribute(mesh.positions, 3));
      g.setIndex(new THREE.BufferAttribute(mesh.indices, 1));
      this.colourFaces(g);
      g.computeBoundingSphere();
      g.computeBoundingBox();
      const mat = new THREE.MeshBasicMaterial({
        vertexColors: true,
        side: THREE.DoubleSide,
        polygonOffset: true,
        polygonOffsetFactor: 1,
        polygonOffsetUnits: 1,
      });
      this.faces = new THREE.Mesh(g, mat);
      this.faces.renderOrder = 2;
      this.scene.add(this.faces);
    }
    if (mesh && mesh.edges.length > 0) {
      const eg = new THREE.BufferGeometry();
      eg.setAttribute("position", new THREE.BufferAttribute(mesh.edges, 3));
      eg.computeBoundingBox();
      this.edges = new THREE.LineSegments(eg, new THREE.LineBasicMaterial({ color: this.theme.edge }));
      this.edges.renderOrder = 3;
      this.scene.add(this.edges);
    }
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

  // ---- highlights and overlays -----------------------------------------------------------

  setHighlight(h: Highlight): void {
    this.highlight = h;
    if (this.faces) this.colourFaces(this.faces.geometry);
    this.rebuildHighlights();
    this.requestRender();
  }

  private fatLines(pairs: Vec3[][], color: string, width: number, dashed = false, depthTest = true, opacity = 1): LineSegments2 {
    const flat: number[] = [];
    for (const [a, b] of pairs) flat.push(...a, ...b);
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

  private rebuildHighlights(): void {
    for (const c of [...this.highlightGroup.children]) disposeObject(c);
    const h = this.highlight;
    if (h.hoverEdges.length) {
      const l = this.fatLines(h.hoverEdges, this.theme.hover, 2.5, false, true, 0.55);
      l.renderOrder = 4;
      this.highlightGroup.add(l);
    }
    if (h.selectedEdges.length) {
      const l = this.fatLines(h.selectedEdges, this.theme.sel, 3, false, true);
      l.renderOrder = 5;
      this.highlightGroup.add(l);
    }
    if (h.selectedPoints.length) {
      const g = new THREE.BufferGeometry();
      g.setAttribute("position", new THREE.Float32BufferAttribute(h.selectedPoints.flat(), 3));
      const pts = new THREE.Points(g, new THREE.PointsMaterial({ color: this.theme.sel, size: 8, sizeAttenuation: false, depthTest: false }));
      pts.renderOrder = 6;
      this.highlightGroup.add(pts);
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
      this.renderer.render(this.scene, this.camera);
      for (const cb of this.listeners) cb();
    });
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
    this.controls.dispose();
    disposeObject(this.gridMinor);
    disposeObject(this.gridMajor);
    disposeObject(this.axes);
    this.renderer.dispose();
  }
}
