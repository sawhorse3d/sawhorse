// The passes sent with a render request, drawn from the current three.js view:
//
//   depth  16-bit greyscale PNG. Linear view depth over the model only, normalised so the
//          nearest model point is white (65535) and the farthest is 1/8 grey; the background
//          is 0. fal's depth LoRA takes it as is (its own depth estimator switched off).
//   edges  8-bit greyscale PNG. The model's edges as black lines on white, hidden lines
//          removed by the faces.
//   clay   8-bit greyscale PNG. Flat-shaded clay; Replicate's fallback model estimates its
//          own depth from it.
//
// Nothing else leaves the machine: no model file, no materials, no textures.
import * as THREE from "three";
import type { MeshData } from "../kernel/types";
import { bytesToBase64, encodeGreyPng } from "./png";

export interface CaptureSource {
  renderer: THREE.WebGLRenderer;
  camera: THREE.Camera;
  mesh: MeshData | null;
  size: { w: number; h: number };
}

export interface RenderPasses {
  width: number;
  height: number;
  depthPng: Uint8Array;
  edgesPng: Uint8Array;
  clayPng: Uint8Array;
  /** 8-bit preview of the depth pass (data: URL) for the panel while the render runs. */
  depthPreview: string;
  /** Model pixels in the frame. Zero means the model is out of view. */
  modelPixels: number;
}

export class NothingToRenderError extends Error {}

/** Output size: the view's aspect, long side `long`, both sides multiples of 16. */
export function passSize(w: number, h: number, long = 1024): { width: number; height: number } {
  const aspect = w > 0 && h > 0 ? w / h : 16 / 10;
  let width = aspect >= 1 ? long : long * aspect;
  let height = aspect >= 1 ? long / aspect : long;
  width = Math.max(256, Math.round(width / 16) * 16);
  height = Math.max(256, Math.round(height / 16) * 16);
  return { width, height };
}

/** A camera like the view's, re-aimed at an output of a different aspect. */
export function cameraFor(src: THREE.Camera, aspect: number): THREE.Camera {
  const cam = src.clone();
  if (cam instanceof THREE.PerspectiveCamera) {
    cam.aspect = aspect;
    cam.updateProjectionMatrix();
  } else if (cam instanceof THREE.OrthographicCamera) {
    const half = (cam.top - cam.bottom) / 2;
    const cy = (cam.top + cam.bottom) / 2;
    const cx = (cam.left + cam.right) / 2;
    cam.top = cy + half;
    cam.bottom = cy - half;
    cam.left = cx - half * aspect;
    cam.right = cx + half * aspect;
    cam.updateProjectionMatrix();
  }
  cam.updateMatrixWorld(true);
  return cam;
}

const DEPTH_VERT = /* glsl */ `
varying float vViewZ;
void main() {
  vec4 mv = modelViewMatrix * vec4(position, 1.0);
  vViewZ = -mv.z;
  gl_Position = projectionMatrix * mv;
}`;

// Linear depth in [near, far] packed into 16 bits across R (high byte) and G (low byte).
const DEPTH_FRAG = /* glsl */ `
precision highp float;
uniform float uNear;
uniform float uFar;
varying float vViewZ;
void main() {
  float d = clamp((vViewZ - uNear) / (uFar - uNear), 0.0, 1.0);
  float v = floor(d * 65535.0 + 0.5);
  float hi = floor(v / 256.0);
  float lo = v - hi * 256.0;
  gl_FragColor = vec4(hi / 255.0, lo / 255.0, 0.0, 1.0);
}`;

function readTarget(renderer: THREE.WebGLRenderer, target: THREE.WebGLRenderTarget, w: number, h: number): Uint8Array {
  const px = new Uint8Array(w * h * 4);
  renderer.readRenderTargetPixels(target, 0, 0, w, h, px);
  // WebGL rows run bottom-up; PNG rows top-down.
  const flipped = new Uint8Array(px.length);
  const row = w * 4;
  for (let y = 0; y < h; y++) flipped.set(px.subarray((h - 1 - y) * row, (h - y) * row), y * row);
  return flipped;
}

function draw(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera, target: THREE.WebGLRenderTarget, clear: number, alpha: number): void {
  const prevTarget = renderer.getRenderTarget();
  const prevColor = renderer.getClearColor(new THREE.Color());
  const prevAlpha = renderer.getClearAlpha();
  try {
    renderer.setRenderTarget(target);
    renderer.setClearColor(clear, alpha);
    renderer.clear(true, true, true);
    renderer.render(scene, camera);
  } finally {
    renderer.setRenderTarget(prevTarget);
    renderer.setClearColor(prevColor, prevAlpha);
  }
}

/** View-space depth range of the mesh's bounding box, padded a little. */
function depthRange(geom: THREE.BufferGeometry, camera: THREE.Camera): { near: number; far: number } {
  geom.computeBoundingBox();
  const b = geom.boundingBox!;
  let near = Infinity;
  let far = -Infinity;
  const v = new THREE.Vector3();
  for (const x of [b.min.x, b.max.x]) for (const y of [b.min.y, b.max.y]) for (const z of [b.min.z, b.max.z]) {
    const d = -v.set(x, y, z).applyMatrix4(camera.matrixWorldInverse).z;
    near = Math.min(near, d);
    far = Math.max(far, d);
  }
  const pad = Math.max((far - near) * 0.01, 1e-4);
  return { near: near - pad, far: far + pad };
}

export async function renderPasses(src: CaptureSource, long = 1024): Promise<RenderPasses> {
  const mesh = src.mesh;
  if (!mesh || mesh.indices.length === 0) throw new NothingToRenderError("Draw something first: the render follows the model in the current view.");
  const { width, height } = passSize(src.size.w, src.size.h, long);
  const camera = cameraFor(src.camera, width / height);
  const renderer = src.renderer;

  const geom = new THREE.BufferGeometry();
  geom.setAttribute("position", new THREE.BufferAttribute(mesh.positions, 3));
  geom.setIndex(new THREE.BufferAttribute(mesh.indices, 1));
  const edgeGeom = new THREE.BufferGeometry();
  edgeGeom.setAttribute("position", new THREE.BufferAttribute(mesh.edges, 3));
  const { near, far } = depthRange(geom, camera);

  const exact = new THREE.WebGLRenderTarget(width, height, { depthBuffer: true, type: THREE.UnsignedByteType, format: THREE.RGBAFormat, minFilter: THREE.NearestFilter, magFilter: THREE.NearestFilter, generateMipmaps: false });
  const smooth = new THREE.WebGLRenderTarget(width, height, { depthBuffer: true, samples: 4, type: THREE.UnsignedByteType, format: THREE.RGBAFormat, generateMipmaps: false });
  const disposables: { dispose(): void }[] = [geom, edgeGeom, exact, smooth];
  try {
    // ---- depth ----
    const depthMat = new THREE.ShaderMaterial({ vertexShader: DEPTH_VERT, fragmentShader: DEPTH_FRAG, uniforms: { uNear: { value: near }, uFar: { value: far } }, side: THREE.DoubleSide });
    disposables.push(depthMat);
    const depthScene = new THREE.Scene();
    depthScene.add(new THREE.Mesh(geom, depthMat));
    draw(renderer, depthScene, camera, exact, 0x000000, 0);
    const dpx = readTarget(renderer, exact, width, height);
    let dmin = 65535;
    let dmax = 0;
    let modelPixels = 0;
    for (let i = 0; i < width * height; i++) {
      if (dpx[i * 4 + 3] === 0) continue;
      const d = dpx[i * 4] * 256 + dpx[i * 4 + 1];
      modelPixels++;
      if (d < dmin) dmin = d;
      if (d > dmax) dmax = d;
    }
    if (modelPixels === 0) throw new NothingToRenderError("The model is out of view. Zoom extents (Shift+Z) and render again.");
    const span = Math.max(1, dmax - dmin);
    const depth = new Uint16Array(width * height);
    const preview = new Uint8ClampedArray(width * height * 4);
    const FLOOR = 8192; // farthest model point: 1/8 grey, so it never merges with the background
    for (let i = 0; i < width * height; i++) {
      let v = 0;
      if (dpx[i * 4 + 3] !== 0) {
        const d = dpx[i * 4] * 256 + dpx[i * 4 + 1];
        v = Math.round(FLOOR + ((dmax - d) / span) * (65535 - FLOOR));
      }
      depth[i] = v;
      const g = v >> 8;
      preview[i * 4] = g;
      preview[i * 4 + 1] = g;
      preview[i * 4 + 2] = g;
      preview[i * 4 + 3] = 255;
    }

    // ---- edges: black lines on white, faces occlude ----
    const occluder = new THREE.MeshBasicMaterial({ color: 0xffffff, side: THREE.DoubleSide, polygonOffset: true, polygonOffsetFactor: 1, polygonOffsetUnits: 1 });
    const ink = new THREE.LineBasicMaterial({ color: 0x000000 });
    disposables.push(occluder, ink);
    const edgeScene = new THREE.Scene();
    edgeScene.add(new THREE.Mesh(geom, occluder), new THREE.LineSegments(edgeGeom, ink));
    draw(renderer, edgeScene, camera, smooth, 0xffffff, 1);
    const epx = readTarget(renderer, smooth, width, height);
    const edges = new Uint8Array(width * height);
    for (let i = 0; i < width * height; i++) edges[i] = epx[i * 4];

    // ---- clay: flat-shaded grey under a key light from the upper left of the view ----
    const clayMat = new THREE.MeshStandardMaterial({ color: 0xd9d4ca, roughness: 1, metalness: 0, flatShading: true, side: THREE.DoubleSide });
    disposables.push(clayMat);
    const clayScene = new THREE.Scene();
    clayScene.add(new THREE.Mesh(geom, clayMat));
    clayScene.add(new THREE.HemisphereLight(0xffffff, 0x8a8a8a, 1.6));
    const key = new THREE.DirectionalLight(0xffffff, 2.2);
    key.position.copy(new THREE.Vector3(-0.6, 0.8, 1).applyQuaternion(camera.quaternion).add(camera.position));
    key.target.position.copy(camera.position).add(new THREE.Vector3(0, 0, -1).applyQuaternion(camera.quaternion));
    clayScene.add(key, key.target);
    draw(renderer, clayScene, camera, smooth, 0xf2f1ee, 1);
    const cpx = readTarget(renderer, smooth, width, height);
    const clay = new Uint8Array(width * height);
    for (let i = 0; i < width * height; i++) clay[i] = Math.round(0.299 * cpx[i * 4] + 0.587 * cpx[i * 4 + 1] + 0.114 * cpx[i * 4 + 2]);

    const [depthPng, edgesPng, clayPng] = await Promise.all([
      encodeGreyPng({ width, height, data: depth }),
      encodeGreyPng({ width, height, data: edges }),
      encodeGreyPng({ width, height, data: clay }),
    ]);
    return { width, height, depthPng, edgesPng, clayPng, depthPreview: previewUrl(preview, width, height), modelPixels };
  } finally {
    for (const d of disposables) d.dispose();
  }
}

function previewUrl(rgba: Uint8ClampedArray, w: number, h: number): string {
  try {
    const c = document.createElement("canvas");
    c.width = w;
    c.height = h;
    const ctx = c.getContext("2d");
    if (!ctx) return "";
    ctx.putImageData(new ImageData(rgba as Uint8ClampedArray<ArrayBuffer>, w, h), 0, 0);
    return c.toDataURL("image/png");
  } catch {
    return "";
  }
}

export function passesToRequest(p: RenderPasses): { depth_png: string; edges_png: string; clay_png: string } {
  return { depth_png: bytesToBase64(p.depthPng), edges_png: bytesToBase64(p.edgesPng), clay_png: bytesToBase64(p.clayPng) };
}
