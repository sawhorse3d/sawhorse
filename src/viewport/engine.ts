// three.js viewport (WebGL2). Z-up, metres, exactly the kernel's frame.
//
// Rendering is on demand (camera change, mesh change, theme change, resize);
// there is no animation loop burning a laptop battery while nothing moves.
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import type { MeshData } from "../kernel/types";
import { classifyHover, type HoverHit, type ScreenSegment } from "../lib/hoverInference";

export type ViewId = "iso" | "top" | "front" | "right" | "persp";

export const VIEWS: { id: ViewId; label: string }[] = [
  { id: "iso", label: "Iso" },
  { id: "top", label: "Top" },
  { id: "front", label: "Front" },
  { id: "right", label: "Right" },
  { id: "persp", label: "Persp" },
];

/** Camera direction (from target toward the eye) and up vector per view. */
const VIEW_DIRS: Record<ViewId, { dir: THREE.Vector3; up: THREE.Vector3 }> = {
  // Isometric feel: eye in the -X/-Y/+Z octant so +X runs right and +Y recedes left, Z up.
  iso: { dir: new THREE.Vector3(-1, -1, 1).normalize(), up: new THREE.Vector3(0, 0, 1) },
  top: { dir: new THREE.Vector3(0, 0, 1), up: new THREE.Vector3(0, 1, 0) },
  front: { dir: new THREE.Vector3(0, -1, 0), up: new THREE.Vector3(0, 0, 1) },
  right: { dir: new THREE.Vector3(1, 0, 0), up: new THREE.Vector3(0, 0, 1) },
  persp: { dir: new THREE.Vector3(-0.9, -1.6, 1).normalize(), up: new THREE.Vector3(0, 0, 1) },
};

/** Axis colours: the conventional red/green/blue, same values as the mock. */
const AXIS_COLORS = ["#c0392b", "#27ae60", "#2a63a8"];

/** Grid: 6 in cells out to 4 ft each way. */
const GRID_STEP = 0.1524;
const GRID_HALF = 8 * GRID_STEP * 2;

interface Theme {
  vp: string;
  grid: string;
  face: string;
  faceTop: string;
  faceSide: string;
  edge: string;
}

function readTheme(el: Element): Theme {
  const cs = getComputedStyle(el);
  const v = (n: string, fallback: string) => cs.getPropertyValue(n).trim() || fallback;
  return {
    vp: v("--vp", "#f4f5f1"),
    grid: v("--vp-grid", "#d4d8cf"),
    face: v("--face", "#e9e4d6"),
    faceTop: v("--face-top", "#f3efe3"),
    faceSide: v("--face-side", "#d9d2c0"),
    edge: v("--edge", "#3a3f3b"),
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

export class ViewportEngine {
  readonly renderer: THREE.WebGLRenderer;
  private scene = new THREE.Scene();
  private ortho = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.01, 1000);
  private persp = new THREE.PerspectiveCamera(35, 1, 0.01, 1000);
  private camera: THREE.OrthographicCamera | THREE.PerspectiveCamera = this.ortho;
  private controls: OrbitControls;
  private grid: THREE.LineSegments;
  private axes: THREE.LineSegments;
  private faces: THREE.Mesh | null = null;
  private edges: THREE.LineSegments | null = null;
  private edgeData: Float32Array = new Float32Array();
  private raycaster = new THREE.Raycaster();
  private theme: Theme;
  private view: ViewId = "iso";
  /** Half-height of the ortho frustum at zoom 1, metres. */
  private frame = 1;
  private size = { w: 1, h: 1 };
  private queued = false;
  private disposed = false;

  constructor(
    canvas: HTMLCanvasElement,
    private themeRoot: Element = document.documentElement,
  ) {
    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, preserveDrawingBuffer: true });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.theme = readTheme(themeRoot);

    this.grid = this.buildGrid();
    this.axes = this.buildAxes();
    this.scene.add(this.grid, this.axes);

    this.controls = new OrbitControls(this.camera, canvas);
    // Left button belongs to the active tool. Middle orbits (Shift+middle pans), right pans, wheel zooms.
    this.controls.mouseButtons = { LEFT: null, MIDDLE: THREE.MOUSE.ROTATE, RIGHT: THREE.MOUSE.PAN };
    this.controls.zoomToCursor = true;
    this.controls.addEventListener("change", () => this.requestRender());

    this.applyTheme();
    this.setView("iso");
  }

  private buildGrid(): THREE.LineSegments {
    const pts: number[] = [];
    const n = Math.round(GRID_HALF / GRID_STEP);
    for (let i = -n; i <= n; i++) {
      const t = i * GRID_STEP;
      pts.push(t, -GRID_HALF, 0, t, GRID_HALF, 0, -GRID_HALF, t, 0, GRID_HALF, t, 0);
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.Float32BufferAttribute(pts, 3));
    const line = new THREE.LineSegments(g, new THREE.LineBasicMaterial({ color: this.theme.grid }));
    line.renderOrder = 0;
    return line;
  }

  private buildAxes(): THREE.LineSegments {
    const L = GRID_HALF * 1.15;
    const pts = [0, 0, 0, L, 0, 0, 0, 0, 0, 0, L, 0, 0, 0, 0, 0, 0, L * 0.75];
    const cols: number[] = [];
    for (const hex of AXIS_COLORS) {
      const c = new THREE.Color(hex);
      cols.push(c.r, c.g, c.b, c.r, c.g, c.b);
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.Float32BufferAttribute(pts, 3));
    g.setAttribute("color", new THREE.Float32BufferAttribute(cols, 3));
    const line = new THREE.LineSegments(g, new THREE.LineBasicMaterial({ vertexColors: true, depthTest: false }));
    line.renderOrder = 1;
    // Lift a hair off the grid so the axes never z-fight it.
    line.position.z = 1e-5;
    return line;
  }

  /** Re-read CSS tokens (called on theme switches). */
  applyTheme(): void {
    this.theme = readTheme(this.themeRoot);
    this.renderer.setClearColor(new THREE.Color(this.theme.vp), 1);
    (this.grid.material as THREE.LineBasicMaterial).color.set(this.theme.grid);
    if (this.edges) (this.edges.material as THREE.LineBasicMaterial).color.set(this.theme.edge);
    if (this.faces) this.colourFaces(this.faces.geometry);
    this.requestRender();
  }

  /** Colour each triangle by its normal with the mock's three face tokens (top / front / side). */
  private colourFaces(geom: THREE.BufferGeometry): void {
    const pos = geom.getAttribute("position") as THREE.BufferAttribute;
    const index = geom.getIndex();
    if (!index) return;
    const colors = new Float32Array(pos.count * 3);
    const top = new THREE.Color(this.theme.faceTop);
    const front = new THREE.Color(this.theme.face);
    const side = new THREE.Color(this.theme.faceSide);
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
      const col = n.z > 0.5 ? top : n.z < -0.5 ? side : Math.abs(n.y) >= Math.abs(n.x) ? front : side;
      for (const i of [ia, ib, ic]) col.toArray(colors, i * 3);
    }
    geom.setAttribute("color", new THREE.BufferAttribute(colors, 3));
  }

  setMesh(mesh: MeshData | null): void {
    if (this.faces) {
      this.scene.remove(this.faces);
      this.faces.geometry.dispose();
      (this.faces.material as THREE.Material).dispose();
      this.faces = null;
    }
    if (this.edges) {
      this.scene.remove(this.edges);
      this.edges.geometry.dispose();
      (this.edges.material as THREE.Material).dispose();
      this.edges = null;
    }
    this.edgeData = new Float32Array();
    if (mesh && mesh.indices.length > 0) {
      const g = new THREE.BufferGeometry();
      g.setAttribute("position", new THREE.BufferAttribute(mesh.positions, 3));
      g.setIndex(new THREE.BufferAttribute(mesh.indices, 1));
      this.colourFaces(g);
      g.computeBoundingSphere();
      const mat = new THREE.MeshBasicMaterial({
        vertexColors: true,
        side: THREE.FrontSide,
        polygonOffset: true,
        polygonOffsetFactor: 1,
        polygonOffsetUnits: 1,
      });
      this.faces = new THREE.Mesh(g, mat);
      this.faces.renderOrder = 2;
      const eg = new THREE.BufferGeometry();
      eg.setAttribute("position", new THREE.BufferAttribute(mesh.edges, 3));
      this.edges = new THREE.LineSegments(eg, new THREE.LineBasicMaterial({ color: this.theme.edge }));
      this.edges.renderOrder = 3;
      this.edgeData = mesh.edges;
      this.scene.add(this.faces, this.edges);
    }
    this.fit();
  }

  /** Bounding box of everything worth framing: the model, or a default 2 ft working area. */
  private contentBox(): THREE.Box3 {
    const box = new THREE.Box3(new THREE.Vector3(-0.15, -0.15, 0), new THREE.Vector3(0.6, 0.45, 0.3));
    if (this.faces) {
      this.faces.geometry.computeBoundingBox();
      const b = this.faces.geometry.boundingBox;
      if (b) box.union(b);
    }
    return box;
  }

  setView(view: ViewId): void {
    this.view = view;
    this.camera = view === "persp" ? this.persp : this.ortho;
    this.controls.object = this.camera;
    this.fit();
  }

  /** Frame the content for the current view. */
  fit(): void {
    const box = this.contentBox();
    const sphere = box.getBoundingSphere(new THREE.Sphere());
    const { dir, up } = VIEW_DIRS[this.view];
    const r = Math.max(sphere.radius, 0.05);
    const target = sphere.center;
    this.camera.up.copy(up);
    if (this.camera instanceof THREE.PerspectiveCamera) {
      const dist = (r * 1.35) / Math.sin(THREE.MathUtils.degToRad(this.camera.fov / 2));
      this.camera.position.copy(target).addScaledVector(dir, dist);
      this.camera.near = Math.max(dist / 1000, 0.001);
      this.camera.far = dist * 50;
    } else {
      this.frame = r * 1.25;
      this.camera.zoom = 1;
      this.camera.position.copy(target).addScaledVector(dir, r * 20);
      this.camera.near = r * 0.5;
      this.camera.far = r * 60;
    }
    this.controls.target.copy(target);
    this.updateProjection();
    this.controls.update();
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

  resize(w: number, h: number): void {
    this.size = { w: Math.max(1, w), h: Math.max(1, h) };
    this.renderer.setSize(this.size.w, this.size.h, false);
    this.updateProjection();
    this.requestRender();
  }

  requestRender(): void {
    if (this.queued || this.disposed) return;
    this.queued = true;
    requestAnimationFrame(() => {
      this.queued = false;
      if (!this.disposed) this.renderer.render(this.scene, this.camera);
    });
  }

  /** Project a world point to canvas CSS pixels. */
  private toScreen(v: THREE.Vector3): [number, number] {
    const p = v.clone().project(this.camera);
    return [((p.x + 1) / 2) * this.size.w, ((1 - p.y) / 2) * this.size.h];
  }

  /** Coarse hover inference at canvas CSS pixel (x, y). */
  hover(x: number, y: number): HoverHit | null {
    if (!this.faces) return null;
    const segs: ScreenSegment[] = [];
    const a = new THREE.Vector3();
    const b = new THREE.Vector3();
    const mid = new THREE.Vector3();
    const e = this.edgeData;
    for (let i = 0; i + 5 < e.length; i += 6) {
      a.set(e[i], e[i + 1], e[i + 2]);
      b.set(e[i + 3], e[i + 4], e[i + 5]);
      // Skip edges hidden behind a face nearer the eye (midpoint occlusion test).
      mid.addVectors(a, b).multiplyScalar(0.5);
      if (this.occluded(mid)) continue;
      segs.push({ a: this.toScreen(a), b: this.toScreen(b) });
    }
    this.raycaster.setFromCamera(this.ndc(x, y), this.camera);
    const hit = this.raycaster.intersectObject(this.faces, false)[0];
    return classifyHover([x, y], segs, hit ? [x, y] : null);
  }

  /** Screen positions (canvas CSS px) of visible edge endpoints: a test and capture hook. */
  visibleEndpoints(): [number, number][] {
    const out: [number, number][] = [];
    const e = this.edgeData;
    const p = new THREE.Vector3();
    for (let i = 0; i + 2 < e.length; i += 3) {
      p.set(e[i], e[i + 1], e[i + 2]);
      if (!this.occluded(p)) out.push(this.toScreen(p));
    }
    return out;
  }

  private ndc(x: number, y: number): THREE.Vector2 {
    return new THREE.Vector2((x / this.size.w) * 2 - 1, -(y / this.size.h) * 2 + 1);
  }

  private occluded(p: THREE.Vector3): boolean {
    if (!this.faces) return false;
    const [sx, sy] = this.toScreen(p);
    this.raycaster.setFromCamera(this.ndc(sx, sy), this.camera);
    const hit = this.raycaster.intersectObject(this.faces, false)[0];
    if (!hit) return false;
    const d = this.raycaster.ray.origin.distanceTo(p);
    const eps = Math.max(1e-4, (this.faces.geometry.boundingSphere?.radius ?? 1) * 1e-3);
    return hit.distance < d - eps;
  }

  dispose(): void {
    this.disposed = true;
    this.setMesh(null);
    this.controls.dispose();
    this.grid.geometry.dispose();
    this.axes.geometry.dispose();
    this.renderer.dispose();
  }
}
