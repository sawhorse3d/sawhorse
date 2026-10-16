// Lane 21 (issue #5): the camera controller on real DOM events (jsdom). The point under the
// cursor at a middle press stays under it for the whole orbit, a right-drag keeps the
// grabbed point under the cursor, the wheel zooms toward the cursor, Shift switches orbit
// and pan mid-drag, a release we never heard ends the drag, touch orbits and pinches, and
// none of it asks for pointer capture.
import * as THREE from "three";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Vec3 } from "./camera";
import { CameraController } from "./controls";

const W = 800;
const H = 600;

/** The model under test: a ground plane and a box top at z = 0.2 over 0 <= x, y <= 0.4. */
function hitAt(cam: THREE.Camera, x: number, y: number): Vec3 | null {
  const rc = new THREE.Raycaster();
  rc.setFromCamera(new THREE.Vector2((x / W) * 2 - 1, 1 - (y / H) * 2), cam);
  const top = rc.ray.intersectPlane(new THREE.Plane(new THREE.Vector3(0, 0, 1), -0.2), new THREE.Vector3());
  if (top && top.x >= 0 && top.x <= 0.4 && top.y >= 0 && top.y <= 0.4) return [top.x, top.y, top.z];
  const g = rc.ray.intersectPlane(new THREE.Plane(new THREE.Vector3(0, 0, 1), 0), new THREE.Vector3());
  return g ? [g.x, g.y, g.z] : null;
}

function screenOf(cam: THREE.Camera, p: Vec3): [number, number] {
  cam.updateMatrixWorld();
  const v = new THREE.Vector3(...p).project(cam);
  return [((v.x + 1) / 2) * W, ((1 - v.y) / 2) * H];
}

function pointer(el: EventTarget, type: string, init: { x: number; y: number; button?: number; buttons?: number; shift?: boolean; id?: number; kind?: string }) {
  const e = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: init.x, clientY: init.y, button: init.button ?? 0, buttons: init.buttons ?? 0, shiftKey: !!init.shift });
  Object.defineProperty(e, "pointerId", { value: init.id ?? 1 });
  Object.defineProperty(e, "pointerType", { value: init.kind ?? "mouse" });
  el.dispatchEvent(e);
  return e;
}

let canvas: HTMLCanvasElement;
let cam: THREE.PerspectiveCamera;
let ctl: CameraController;
let navigating: ReturnType<typeof vi.fn<() => void>>;
let changes = 0;

function setup(camera: THREE.PerspectiveCamera | THREE.OrthographicCamera = new THREE.PerspectiveCamera(35, W / H, 0.01, 100)) {
  canvas = document.createElement("canvas");
  document.body.appendChild(canvas);
  canvas.getBoundingClientRect = () => ({ left: 0, top: 0, right: W, bottom: H, width: W, height: H, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect;
  // Pointer capture must not be needed (synthetic events have no live pointer behind them).
  canvas.setPointerCapture = () => {
    throw new Error("setPointerCapture called");
  };
  camera.up.set(0, 0, 1);
  camera.position.set(-0.6, -0.9, 0.8);
  camera.lookAt(0.2, 0.2, 0.1);
  camera.updateMatrixWorld();
  cam = camera as THREE.PerspectiveCamera;
  navigating = vi.fn<() => void>();
  ctl = new CameraController(camera, canvas, { pick: (x, y) => hitAt(camera, x, y), size: () => ({ w: W, h: H }), navigating });
  ctl.target.set(0.2, 0.2, 0.1);
  changes = 0;
  ctl.onChange(() => changes++);
}

beforeEach(() => setup());
afterEach(() => {
  ctl.dispose();
  canvas.remove();
});

describe("middle-drag orbit (issue #5)", () => {
  it("pins the point under the cursor at the press, shows the pivot while held, and moves the look-at point", () => {
    const at: [number, number] = [470, 260];
    const pivot = hitAt(cam, ...at)!;
    expect(pivot[2]).toBeCloseTo(0.2, 9); // on the box top
    pointer(canvas, "pointerdown", { x: at[0], y: at[1], button: 1, buttons: 4 });
    expect(ctl.state().mode).toBe("orbit");
    expect(ctl.state().pivot).toEqual(pivot);
    expect(navigating).toHaveBeenCalledTimes(1);
    let worst = 0;
    // 200 px across, then 200 px down, in 10 px steps (moves bubble to the document).
    for (let i = 1; i <= 40; i++) {
      const dx = i <= 20 ? 10 * i : 200;
      const dy = i <= 20 ? 0 : 10 * (i - 20);
      pointer(canvas, "pointermove", { x: at[0] + dx, y: at[1] + dy, buttons: 4 });
      const s = screenOf(cam, pivot);
      worst = Math.max(worst, Math.hypot(s[0] - at[0], s[1] - at[1]));
    }
    expect(worst).toBeLessThan(1e-6);
    // Every step moved the camera until the view reached 89.5 degrees down; the steps past
    // that changed nothing and were not reported.
    expect(changes).toBeGreaterThan(30);
    expect(changes).toBeLessThan(40);
    // The look-at point is on the new view direction, at the pivot's depth.
    const fwd = new THREE.Vector3();
    cam.getWorldDirection(fwd);
    const toT = ctl.target.clone().sub(cam.position);
    expect(toT.clone().normalize().dot(fwd)).toBeCloseTo(1, 12);
    expect(toT.dot(fwd)).toBeCloseTo(new THREE.Vector3(...pivot).sub(cam.position).dot(fwd), 9);
    expect(cam.up.toArray()).toEqual([0, 0, 1]);
    pointer(canvas, "pointerup", { x: at[0] + 200, y: at[1] + 200, button: 1 });
    expect(ctl.state()).toEqual({ mode: null, pivot: null });
    expect(navigating).toHaveBeenCalledTimes(2);
    // Moves after the release do nothing.
    const before = cam.position.clone();
    pointer(canvas, "pointermove", { x: 10, y: 10, buttons: 0 });
    expect(cam.position.equals(before)).toBe(true);
  });

  it("orbits about the look-at point when the cursor ray meets nothing", () => {
    ctl.dispose();
    canvas.remove();
    setup();
    // A host whose pick always misses.
    ctl.dispose();
    ctl = new CameraController(cam, canvas, { pick: () => null, size: () => ({ w: W, h: H }), navigating });
    ctl.target.set(0.2, 0.2, 0.1);
    pointer(canvas, "pointerdown", { x: 30, y: 30, button: 1, buttons: 4 });
    expect(ctl.state().pivot).toEqual([0.2, 0.2, 0.1]);
    const s0 = screenOf(cam, [0.2, 0.2, 0.1]);
    pointer(canvas, "pointermove", { x: 130, y: 90, buttons: 4 });
    const s1 = screenOf(cam, [0.2, 0.2, 0.1]);
    expect(Math.hypot(s1[0] - s0[0], s1[1] - s0[1])).toBeLessThan(1e-6);
    pointer(canvas, "pointerup", { x: 130, y: 90, button: 1 });
  });

  it("a vertical drag far past the pole stops short of it, and the view still turns", () => {
    pointer(canvas, "pointerdown", { x: 400, y: 300, button: 1, buttons: 4 });
    for (let i = 1; i <= 100; i++) pointer(canvas, "pointermove", { x: 400, y: 300 + 20 * i, buttons: 4 });
    const fwd = new THREE.Vector3();
    cam.getWorldDirection(fwd);
    expect(-fwd.z).toBeCloseTo(Math.sin((89.5 * Math.PI) / 180), 9);
    const right0 = new THREE.Vector3().setFromMatrixColumn(cam.matrixWorld, 0);
    pointer(canvas, "pointermove", { x: 600, y: 2300, buttons: 4 });
    const right1 = new THREE.Vector3().setFromMatrixColumn(cam.matrixWorld, 0);
    expect(right0.dot(right1)).toBeLessThan(Math.cos(1));
    pointer(canvas, "pointerup", { x: 600, y: 2300, button: 1 });
  });

  it("ends the drag when the button turns out to have been released elsewhere", () => {
    pointer(canvas, "pointerdown", { x: 400, y: 300, button: 1, buttons: 4 });
    pointer(canvas, "pointermove", { x: 420, y: 300, buttons: 4 });
    const pos = cam.position.clone();
    pointer(canvas, "pointermove", { x: 480, y: 300, buttons: 0 });
    expect(ctl.state().mode).toBeNull();
    expect(cam.position.equals(pos)).toBe(true);
  });

  it("Shift+middle pans, and Shift pressed mid-drag switches an orbit to a pan", () => {
    const at: [number, number] = [300, 380];
    const grabbed = hitAt(cam, ...at)!;
    pointer(canvas, "pointerdown", { x: at[0], y: at[1], button: 1, buttons: 4, shift: true });
    expect(ctl.state().mode).toBe("pan");
    expect(ctl.state().pivot).toBeNull();
    pointer(canvas, "pointermove", { x: at[0] - 90, y: at[1] + 40, buttons: 4, shift: true });
    let s = screenOf(cam, grabbed);
    expect(Math.hypot(s[0] - (at[0] - 90), s[1] - (at[1] + 40))).toBeLessThan(1e-6);
    pointer(canvas, "pointerup", { x: at[0] - 90, y: at[1] + 40, button: 1 });
    // Orbit, then Shift: the pan holds the point under the cursor where Shift went down.
    pointer(canvas, "pointerdown", { x: 400, y: 300, button: 1, buttons: 4 });
    pointer(canvas, "pointermove", { x: 430, y: 300, buttons: 4 });
    expect(ctl.state().mode).toBe("orbit");
    const under = hitAt(cam, 450, 310)!;
    pointer(canvas, "pointermove", { x: 450, y: 310, buttons: 4, shift: true });
    expect(ctl.state().mode).toBe("pan");
    pointer(canvas, "pointermove", { x: 380, y: 360, buttons: 4, shift: true });
    s = screenOf(cam, under);
    expect(Math.hypot(s[0] - 380, s[1] - 360)).toBeLessThan(1e-6);
    pointer(canvas, "pointerup", { x: 380, y: 360, button: 1 });
  });
});

describe("right-drag pan and wheel zoom", () => {
  it("right-drag keeps the grabbed point under the cursor and the view direction", () => {
    const at: [number, number] = [520, 300];
    const grabbed = hitAt(cam, ...at)!;
    const fwd0 = new THREE.Vector3();
    cam.getWorldDirection(fwd0);
    pointer(canvas, "pointerdown", { x: at[0], y: at[1], button: 2, buttons: 2 });
    for (const [x, y] of [
      [500, 320],
      [300, 410],
      [150, 120],
    ]) {
      pointer(canvas, "pointermove", { x, y, buttons: 2 });
      const s = screenOf(cam, grabbed);
      expect(Math.hypot(s[0] - x, s[1] - y)).toBeLessThan(1e-6);
    }
    const fwd1 = new THREE.Vector3();
    cam.getWorldDirection(fwd1);
    expect(fwd0.dot(fwd1)).toBeCloseTo(1, 12);
    pointer(canvas, "pointerup", { x: 150, y: 120, button: 2 });
    expect(ctl.state().mode).toBeNull();
  });

  it("the wheel zooms toward the cursor in perspective and in parallel", () => {
    for (const camera of [new THREE.PerspectiveCamera(35, W / H, 0.01, 100), new THREE.OrthographicCamera(-0.5 * (W / H), 0.5 * (W / H), 0.5, -0.5, -100, 100)]) {
      ctl.dispose();
      canvas.remove();
      setup(camera);
      const at: [number, number] = [250, 420];
      const under = hitAt(camera, ...at)!;
      const before = camera.position.distanceTo(new THREE.Vector3(...under));
      const zoom0 = camera.zoom;
      const e = new WheelEvent("wheel", { bubbles: true, cancelable: true, clientX: at[0], clientY: at[1], deltaY: -400 });
      canvas.dispatchEvent(e);
      expect(e.defaultPrevented).toBe(true);
      const s = screenOf(camera, under);
      expect(Math.hypot(s[0] - at[0], s[1] - at[1])).toBeLessThan(1e-6);
      if (camera instanceof THREE.PerspectiveCamera) expect(camera.position.distanceTo(new THREE.Vector3(...under))).toBeCloseTo(before * Math.pow(0.95, 4), 9);
      else expect(camera.zoom).toBeCloseTo(zoom0 / Math.pow(0.95, 4), 12);
    }
  });
});

describe("touch", () => {
  it("one finger orbits about the point under it; two fingers pinch about their midpoint", () => {
    const at: [number, number] = [460, 250];
    const pivot = hitAt(cam, ...at)!;
    pointer(canvas, "pointerdown", { x: at[0], y: at[1], id: 11, kind: "touch" });
    expect(ctl.state().mode).toBe("orbit");
    pointer(canvas, "pointermove", { x: at[0] + 80, y: at[1] + 30, id: 11, kind: "touch" });
    let s = screenOf(cam, pivot);
    expect(Math.hypot(s[0] - at[0], s[1] - at[1])).toBeLessThan(1e-6);
    // A second finger: pinch.
    pointer(canvas, "pointerdown", { x: at[0] + 180, y: at[1] + 30, id: 12, kind: "touch" });
    expect(ctl.state().mode).toBe("pinch");
    const mid: [number, number] = [at[0] + 130, at[1] + 30];
    const under = hitAt(cam, ...mid)!;
    const d0 = cam.position.distanceTo(new THREE.Vector3(...under));
    // Spread the fingers to twice their spacing about the same midpoint.
    pointer(canvas, "pointermove", { x: at[0] + 230, y: at[1] + 30, id: 12, kind: "touch" });
    pointer(canvas, "pointermove", { x: at[0] + 30, y: at[1] + 30, id: 11, kind: "touch" });
    s = screenOf(cam, under);
    expect(Math.hypot(s[0] - mid[0], s[1] - mid[1])).toBeLessThan(1e-6);
    expect(cam.position.distanceTo(new THREE.Vector3(...under))).toBeLessThan(d0 * 0.6);
    pointer(canvas, "pointerup", { x: at[0] + 30, y: at[1] + 30, id: 11, kind: "touch" });
    expect(ctl.state().mode).toBe("orbit");
    pointer(canvas, "pointerup", { x: at[0] + 230, y: at[1] + 30, id: 12, kind: "touch" });
    expect(ctl.state().mode).toBeNull();
  });
});

describe("update", () => {
  it("aims the camera at the target and reports a change only when something moved", () => {
    ctl.target.set(0.4, 0.1, 0);
    expect(ctl.update()).toBe(true);
    expect(ctl.update()).toBe(false);
    const fwd = new THREE.Vector3();
    cam.getWorldDirection(fwd);
    expect(fwd.dot(ctl.target.clone().sub(cam.position).normalize())).toBeCloseTo(1, 12);
  });
});
