// Drawing materials, styles and shadows in three.js (lane 10, ADR-0008). Pure helpers the
// viewport engine composes: the sky and ground backdrop, profile / extension / endpoint edge
// geometry from the kernel's edge normals, texture images decoded once per digest, and
// which faces a style draws how.
import * as THREE from "three";
import type { FullStyle } from "../lib/looks";

/** A full-screen backdrop: background colour, a sky above the horizon and a ground below it.
 * The horizon follows the camera's pitch (in either projection), as a woodworker expects
 * when tilting the view. */
export function makeBackdrop(): THREE.Mesh {
  const mat = new THREE.ShaderMaterial({
    uniforms: {
      bg: { value: new THREE.Color() },
      sky: { value: new THREE.Color() },
      ground: { value: new THREE.Color() },
      useSky: { value: 0 },
      useGround: { value: 0 },
      pitch: { value: 0 },
      halfFov: { value: 0.35 },
    },
    vertexShader: /* glsl */ `
      varying float vy;
      void main() { vy = position.y; gl_Position = vec4(position.xy, 0.9999, 1.0); }`,
    fragmentShader: /* glsl */ `
      uniform vec3 bg; uniform vec3 sky; uniform vec3 ground;
      uniform float useSky; uniform float useGround; uniform float pitch; uniform float halfFov;
      varying float vy;
      void main() {
        float elev = pitch + atan(vy * tan(halfFov));
        vec3 c = bg;
        if (elev >= 0.0) c = mix(bg, sky, useSky * smoothstep(0.0, 0.5, elev));
        else c = mix(bg, ground, useGround * (0.85 + 0.15 * smoothstep(0.0, -0.6, elev)));
        gl_FragColor = vec4(c, 1.0);
        #include <colorspace_fragment>
      }`,
    depthTest: false,
    depthWrite: false,
  });
  const m = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), mat);
  m.frustumCulled = false;
  m.renderOrder = -10;
  return m;
}

export function setBackdrop(m: THREE.Mesh, style: FullStyle, bg: string, camera: THREE.Camera, target: THREE.Vector3): void {
  const u = (m.material as THREE.ShaderMaterial).uniforms;
  u.bg.value.set(style.background ? rgbCss(style.background) : bg);
  u.useSky.value = style.sky ? 1 : 0;
  u.useGround.value = style.ground ? 1 : 0;
  if (style.sky) u.sky.value.set(rgbCss(style.sky));
  if (style.ground) u.ground.value.set(rgbCss(style.ground));
  const fwd = target.clone().sub(camera.position).normalize();
  u.pitch.value = Math.asin(Math.max(-1, Math.min(1, fwd.z)));
  u.halfFov.value = camera instanceof THREE.PerspectiveCamera ? THREE.MathUtils.degToRad(camera.fov / 2) / camera.zoom : THREE.MathUtils.degToRad(17.5);
  m.visible = !!(style.sky || style.ground);
}

export function rgbCss(c: readonly number[]): string {
  return `rgb(${c[0]}, ${c[1]}, ${c[2]})`;
}

/** The view direction towards point `p` (unit, from the eye). */
function viewDir(camera: THREE.Camera, p: THREE.Vector3, out: THREE.Vector3): THREE.Vector3 {
  if (camera instanceof THREE.PerspectiveCamera) return out.copy(p).sub(camera.position).normalize();
  return camera.getWorldDirection(out);
}

/**
 * Profile edges: where one of the two faces at an edge turns towards the eye and the other
 * away (a silhouette), or an edge with one face only (an open boundary). `normals`: six per
 * edge (crates/kernel/src/mesh.rs `edge_normals`); edges with no face at all are strokes and
 * are never profiles. Returns the segments as flat xyz pairs.
 */
export function profileSegments(edges: Float32Array, normals: Float32Array | undefined, camera: THREE.Camera): number[] {
  const out: number[] = [];
  if (!normals || normals.length !== edges.length) return out;
  const mid = new THREE.Vector3();
  const v = new THREE.Vector3();
  for (let i = 0; i + 5 < edges.length; i += 6) {
    const n1 = [normals[i], normals[i + 1], normals[i + 2]];
    const n2 = [normals[i + 3], normals[i + 4], normals[i + 5]];
    const has1 = n1[0] !== 0 || n1[1] !== 0 || n1[2] !== 0;
    const has2 = n2[0] !== 0 || n2[1] !== 0 || n2[2] !== 0;
    if (!has1 && !has2) continue;
    let profile = has1 !== has2;
    if (!profile) {
      mid.set((edges[i] + edges[i + 3]) / 2, (edges[i + 1] + edges[i + 4]) / 2, (edges[i + 2] + edges[i + 5]) / 2);
      viewDir(camera, mid, v);
      const d1 = n1[0] * v.x + n1[1] * v.y + n1[2] * v.z;
      const d2 = n2[0] * v.x + n2[1] * v.y + n2[2] * v.z;
      profile = (d1 < -1e-9 && d2 > 1e-9) || (d1 > 1e-9 && d2 < -1e-9);
    }
    if (profile) for (let k = 0; k < 6; k++) out.push(edges[i + k]);
  }
  return out;
}

/** Every edge run `ext(midpoint)` metres past both ends (pencil overruns). */
export function extendedSegments(edges: Float32Array, ext: (p: THREE.Vector3) => number): Float32Array {
  const out = new Float32Array(edges.length);
  const a = new THREE.Vector3();
  const b = new THREE.Vector3();
  const d = new THREE.Vector3();
  for (let i = 0; i + 5 < edges.length; i += 6) {
    a.set(edges[i], edges[i + 1], edges[i + 2]);
    b.set(edges[i + 3], edges[i + 4], edges[i + 5]);
    d.subVectors(b, a);
    const len = d.length();
    const e = len > 0 ? ext(a.clone().add(b).multiplyScalar(0.5)) : 0;
    if (len > 0) d.multiplyScalar(e / len);
    a.sub(d);
    b.add(d);
    out.set([a.x, a.y, a.z, b.x, b.y, b.z], i);
  }
  return out;
}

/** The last `len(midpoint)` metres at both ends of every edge (endpoints drawn heavier),
 * never more than a third of the edge. */
export function endpointSegments(edges: Float32Array, len: (p: THREE.Vector3) => number): number[] {
  const out: number[] = [];
  const a = new THREE.Vector3();
  const b = new THREE.Vector3();
  const d = new THREE.Vector3();
  for (let i = 0; i + 5 < edges.length; i += 6) {
    a.set(edges[i], edges[i + 1], edges[i + 2]);
    b.set(edges[i + 3], edges[i + 4], edges[i + 5]);
    d.subVectors(b, a);
    const full = d.length();
    if (full <= 0) continue;
    const l = Math.min(full / 3, len(a.clone().add(b).multiplyScalar(0.5)));
    d.multiplyScalar(l / full);
    out.push(a.x, a.y, a.z, a.x + d.x, a.y + d.y, a.z + d.z);
    out.push(b.x - d.x, b.y - d.y, b.z - d.z, b.x, b.y, b.z);
  }
  return out;
}

/** How a face style draws faces and edges. */
export interface FaceMode {
  /** Faces drawn at all. */
  faces: boolean;
  /** Material colours. */
  colours: boolean;
  /** Textures. */
  textures: boolean;
  /** Every face in the background colour. */
  paper: boolean;
  /** See-through faces. */
  xray: boolean;
  /** Edges behind faces drawn too. */
  hiddenEdges: boolean;
}

export function faceMode(style: FullStyle): FaceMode {
  switch (style.face) {
    case "shaded":
      return { faces: true, colours: true, textures: false, paper: false, xray: false, hiddenEdges: false };
    case "monochrome":
      return { faces: true, colours: false, textures: false, paper: false, xray: false, hiddenEdges: false };
    case "hidden_line":
      return { faces: true, colours: false, textures: false, paper: true, xray: false, hiddenEdges: false };
    case "wireframe":
      return { faces: false, colours: false, textures: false, paper: false, xray: false, hiddenEdges: true };
    case "x_ray":
      return { faces: true, colours: true, textures: false, paper: false, xray: true, hiddenEdges: true };
    default:
      return { faces: true, colours: true, textures: true, paper: false, xray: false, hiddenEdges: false };
  }
}

/**
 * Texture images by content digest, decoded once and shared by every material that names
 * them (the worker sends each image's bytes once). Colour space sRGB, repeat wrapping,
 * mipmaps and the renderer's best anisotropy.
 */
export class TextureCache {
  private byDigest = new Map<string, THREE.Texture>();
  private pending = new Set<string>();
  constructor(
    private anisotropy: number,
    private onReady: () => void,
  ) {}

  has(digest: string): boolean {
    return this.byDigest.has(digest);
  }

  get(digest: string | undefined): THREE.Texture | null {
    return digest ? (this.byDigest.get(digest) ?? null) : null;
  }

  add(digest: string, mime: string, bytes: Uint8Array): void {
    if (this.byDigest.has(digest) || this.pending.has(digest)) return;
    this.pending.add(digest);
    const blob = new Blob([bytes as BlobPart], { type: mime });
    const done = (img: TexImageSource | null) => {
      this.pending.delete(digest);
      if (!img) return;
      const t = new THREE.Texture(img as THREE.Texture["image"]);
      t.colorSpace = THREE.SRGBColorSpace;
      t.wrapS = THREE.RepeatWrapping;
      t.wrapT = THREE.RepeatWrapping;
      t.anisotropy = this.anisotropy;
      t.generateMipmaps = true;
      t.minFilter = THREE.LinearMipmapLinearFilter;
      t.magFilter = THREE.LinearFilter;
      // Our coordinates run y up the image's height from its first row.
      t.flipY = false;
      t.needsUpdate = true;
      this.byDigest.set(digest, t);
      this.onReady();
    };
    if (typeof createImageBitmap === "function") {
      createImageBitmap(blob).then(done, () => done(null));
    } else if (typeof Image !== "undefined" && typeof URL !== "undefined" && URL.createObjectURL) {
      const img = new Image();
      const url = URL.createObjectURL(blob);
      img.onload = () => {
        URL.revokeObjectURL(url);
        done(img);
      };
      img.onerror = () => {
        URL.revokeObjectURL(url);
        done(null);
      };
      img.src = url;
    } else {
      this.pending.delete(digest);
    }
  }

  size(): number {
    return this.byDigest.size;
  }

  dispose(): void {
    for (const t of this.byDigest.values()) t.dispose();
    this.byDigest.clear();
  }
}

/** Soft (1), smooth (2) and hidden (4) edge flags (crates/kernel/src/edges.rs). */
export const EDGE_SOFT = 1;
export const EDGE_SMOOTH = 2;
export const EDGE_HIDDEN = 4;

/** The model's edges sorted by how they are drawn (lane 15): `hard` ones as lines; `soft`
 * ones (soft, not hidden) only as profiles where they turn into a silhouette, or dashed with
 * the style's "soft and hidden edges" switch; `hidden` ones only dashed with that switch.
 * Normals follow their edges (six per segment). Without flags, `hard` is the input itself. */
export interface EdgeSets {
  hard: Float32Array;
  hardNormals: Float32Array | undefined;
  soft: Float32Array;
  softNormals: Float32Array;
  hidden: Float32Array;
}

export function edgeSets(edges: Float32Array, normals: Float32Array | undefined, flags: Uint8Array | undefined): EdgeSets {
  const none = new Float32Array(0);
  const n = edges.length / 6;
  if (!flags || flags.length !== n || !flags.some((f) => (f & (EDGE_SOFT | EDGE_HIDDEN)) !== 0)) {
    return { hard: edges, hardNormals: normals, soft: none, softNormals: none, hidden: none };
  }
  const hard: number[] = [];
  const hardN: number[] = [];
  const soft: number[] = [];
  const softN: number[] = [];
  const hidden: number[] = [];
  const withN = !!normals && normals.length === edges.length;
  for (let i = 0; i < n; i++) {
    const seg = edges.subarray(6 * i, 6 * i + 6);
    const nn = withN ? normals!.subarray(6 * i, 6 * i + 6) : null;
    const f = flags[i];
    if (f & EDGE_HIDDEN) hidden.push(...seg);
    else if (f & EDGE_SOFT) {
      soft.push(...seg);
      if (nn) softN.push(...nn);
    } else {
      hard.push(...seg);
      if (nn) hardN.push(...nn);
    }
  }
  return {
    hard: new Float32Array(hard),
    hardNormals: withN ? new Float32Array(hardN) : undefined,
    soft: new Float32Array(soft),
    softNormals: new Float32Array(softN),
    hidden: new Float32Array(hidden),
  };
}

/** Shade weights (top, front, side) for a normal (lane 15): the theme's three face tones,
 * blended smoothly so a smoothed surface turns from one to the next. A normal along an axis
 * gives exactly the tone a flat face of that direction gets. */
export function toneWeights(x: number, y: number, z: number): [number, number, number] {
  const top = Math.max(0, z);
  const bottom = Math.max(0, -z);
  const h = Math.max(0, 1 - top - bottom);
  const yy = y * y;
  const xx = x * x;
  const front = xx + yy > 1e-12 ? (h * yy) / (xx + yy) : 0;
  return [top, front, h - front + bottom];
}
