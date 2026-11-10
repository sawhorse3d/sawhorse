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

function pointer(el: EventTarget, type: string, init: { x: number; y: number; button?: number; buttons?: number; shift?: boolean; alt?: boolean; ctrl?: boolean; meta?: boolean; id?: number; kind?: string }) {
  const e = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: init.x, clientY: init.y, button: init.button ?? 0, buttons: init.buttons ?? 0, shiftKey: !!init.shift, altKey: !!init.alt, ctrlKey: !!init.ctrl, metaKey: !!init.meta });
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
/** Lane 30: what the app says a left press does (unset: the tool's, as before lane 30). */
let leftButton: ((mods: { alt: boolean; shift: boolean; macControl?: boolean }) => "orbit" | "pan" | null) | undefined;

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
  ctl = new CameraController(camera, canvas, { pick: (x, y) => hitAt(camera, x, y), size: () => ({ w: W, h: H }), navigating, leftButton: (mods) => leftButton?.(mods) ?? null });
  ctl.target.set(0.2, 0.2, 0.1);
  changes = 0;
  ctl.onChange(() => changes++);
}

beforeEach(() => {
  leftButton = undefined;
  setup();
});
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

describe("left-button orbit and pan (lane 30, issue #36: a trackpad has no middle button)", () => {
  /** The app's rule in a tool without an Alt meaning (src/tools/navigate.ts). */
  const altTool = (m: { alt: boolean; shift: boolean }) => (m.alt ? (m.shift ? "pan" : "orbit") : null);
  /** Under the Orbit tool. */
  const orbitTool = (m: { alt: boolean; shift: boolean }) => (m.shift ? "pan" : "orbit");

  /** Orbit 120 px across and 80 px down from `at`, then let go; the worst slip of the pivot
   * on screen and the pose reached. */
  function orbitFrom(at: [number, number], press: { button: number; buttons: number; alt?: boolean }) {
    const pivot = ctl.state().pivot!;
    let worst = 0;
    for (let i = 1; i <= 20; i++) {
      pointer(canvas, "pointermove", { x: at[0] + 6 * i, y: at[1] + 4 * i, buttons: press.buttons, alt: press.alt });
      const s = screenOf(cam, pivot);
      worst = Math.max(worst, Math.hypot(s[0] - at[0], s[1] - at[1]));
    }
    pointer(canvas, "pointerup", { x: at[0] + 120, y: at[1] + 80, button: press.button, alt: press.alt });
    return { worst, pose: [...cam.position.toArray(), ...cam.quaternion.toArray(), ...ctl.target.toArray()] };
  }

  it("Alt+left orbits about the same pivot as a middle press, the same way, and ends on its release", () => {
    leftButton = altTool;
    const at: [number, number] = [470, 260];
    // The middle button first: its pivot and where 20 steps take the camera.
    pointer(canvas, "pointerdown", { x: at[0], y: at[1], button: 1, buttons: 4 });
    const middlePivot = ctl.state().pivot;
    expect(middlePivot).toEqual(hitAt(cam, ...at));
    const middle = orbitFrom(at, { button: 1, buttons: 4 });
    expect(middle.worst).toBeLessThan(1e-6);
    // The same from the same starting camera with Alt and the left button.
    ctl.dispose();
    canvas.remove();
    setup();
    const down = pointer(canvas, "pointerdown", { x: at[0], y: at[1], button: 0, buttons: 1, alt: true });
    expect(down.defaultPrevented).toBe(true);
    expect(ctl.state()).toEqual({ mode: "orbit", pivot: middlePivot });
    expect(navigating).toHaveBeenCalledTimes(1);
    const left = orbitFrom(at, { button: 0, buttons: 1, alt: true });
    expect(left.worst).toBeLessThan(1e-6);
    expect(left.pose).toEqual(middle.pose);
    expect(ctl.state()).toEqual({ mode: null, pivot: null });
    expect(navigating).toHaveBeenCalledTimes(2);
  });

  it("Alt+Shift+left pans, keeping the grabbed point under the cursor, and letting Shift go orbits", () => {
    leftButton = altTool;
    const at: [number, number] = [300, 380];
    const grabbed = hitAt(cam, ...at)!;
    pointer(canvas, "pointerdown", { x: at[0], y: at[1], button: 0, buttons: 1, alt: true, shift: true });
    expect(ctl.state().mode).toBe("pan");
    pointer(canvas, "pointermove", { x: at[0] - 90, y: at[1] + 40, buttons: 1, alt: true, shift: true });
    const s = screenOf(cam, grabbed);
    expect(Math.hypot(s[0] - (at[0] - 90), s[1] - (at[1] + 40))).toBeLessThan(1e-6);
    // Shift let go, and Alt too: the drag keeps the Alt it started with.
    pointer(canvas, "pointermove", { x: at[0] - 80, y: at[1] + 40, buttons: 1 });
    expect(ctl.state().mode).toBe("orbit");
    pointer(canvas, "pointerup", { x: at[0] - 80, y: at[1] + 40, button: 0 });
    expect(ctl.state().mode).toBeNull();
  });

  it("Shift switches a drag both ways as often as it is pressed, and the release ends it, with moves that carry no button (as browsers send them)", () => {
    for (const [press, rule] of [
      [{ button: 1, buttons: 4 }, undefined],
      [{ button: 0, buttons: 1, alt: true }, altTool],
    ] as const) {
      leftButton = rule;
      pointer(canvas, "pointerdown", { x: 400, y: 300, ...press });
      expect(ctl.state().mode).toBe("orbit");
      const modes: (string | null)[] = [];
      [true, false, true, false].forEach((shift, i) => {
        // A pointermove's `button` is -1 in a browser.
        pointer(canvas, "pointermove", { x: 410 + 10 * i, y: 300, button: -1, buttons: press.buttons, shift, alt: "alt" in press });
        modes.push(ctl.state().mode);
      });
      expect(modes).toEqual(["pan", "orbit", "pan", "orbit"]);
      pointer(canvas, "pointerup", { x: 450, y: 300, button: press.button });
      expect(ctl.state().mode).toBeNull();
    }
  });

  it("a left press without Alt in a tool is the tool's: not taken, nothing moves", () => {
    leftButton = altTool;
    const before = [...cam.position.toArray(), ...cam.quaternion.toArray()];
    for (const shift of [false, true]) {
      const down = pointer(canvas, "pointerdown", { x: 400, y: 300, button: 0, buttons: 1, shift });
      expect(down.defaultPrevented).toBe(false);
      expect(ctl.state().mode).toBeNull();
      pointer(canvas, "pointermove", { x: 480, y: 340, buttons: 1, shift });
      pointer(canvas, "pointerup", { x: 480, y: 340, button: 0, shift });
    }
    expect([...cam.position.toArray(), ...cam.quaternion.toArray()]).toEqual(before);
    expect(changes).toBe(0);
    expect(navigating).not.toHaveBeenCalled();
  });

  it("with no rule from the host the left button is never taken, Alt or not (as before lane 30)", () => {
    const down = pointer(canvas, "pointerdown", { x: 400, y: 300, button: 0, buttons: 1, alt: true });
    expect(down.defaultPrevented).toBe(false);
    expect(ctl.state().mode).toBeNull();
  });

  it("under the Orbit tool a plain left press orbits about the point under it, and Shift pans", () => {
    leftButton = orbitTool;
    const at: [number, number] = [460, 250];
    const pivot = hitAt(cam, ...at)!;
    pointer(canvas, "pointerdown", { x: at[0], y: at[1], button: 0, buttons: 1 });
    expect(ctl.state()).toEqual({ mode: "orbit", pivot });
    const r = orbitFrom(at, { button: 0, buttons: 1 });
    expect(r.worst).toBeLessThan(1e-6);
    expect(changes).toBeGreaterThan(10);
    // Shift held at the press pans, as Shift+middle does.
    pointer(canvas, "pointerdown", { x: 400, y: 300, button: 0, buttons: 1, shift: true });
    expect(ctl.state().mode).toBe("pan");
    pointer(canvas, "pointerup", { x: 400, y: 300, button: 0, shift: true });
    // A release we never heard ends a left drag too.
    pointer(canvas, "pointerdown", { x: 400, y: 300, button: 0, buttons: 1 });
    pointer(canvas, "pointermove", { x: 420, y: 300, buttons: 0 });
    expect(ctl.state().mode).toBeNull();
  });

  it("under the Pan tool a left drag pans whatever Shift does", () => {
    leftButton = () => "pan";
    const at: [number, number] = [520, 300];
    const grabbed = hitAt(cam, ...at)!;
    pointer(canvas, "pointerdown", { x: at[0], y: at[1], button: 0, buttons: 1 });
    expect(ctl.state().mode).toBe("pan");
    pointer(canvas, "pointermove", { x: 300, y: 410, buttons: 1, shift: true });
    expect(ctl.state().mode).toBe("pan");
    const s = screenOf(cam, grabbed);
    expect(Math.hypot(s[0] - 300, s[1] - 410)).toBeLessThan(1e-6);
    pointer(canvas, "pointerup", { x: 300, y: 410, button: 0 });
  });

  it("macOS: a Control+left press is the right click and pans, through Control let go and Shift, until its release; elsewhere and with Cmd it is a left press", () => {
    // The app's rule under the Orbit tool, the Mac's Control+click first (src/tools/navigate.ts).
    leftButton = (m) => (m.macControl ? "pan" : orbitTool(m));
    const platform = vi.spyOn(navigator, "platform", "get");
    try {
      platform.mockReturnValue("Win32");
      pointer(canvas, "pointerdown", { x: 400, y: 300, button: 0, buttons: 1, ctrl: true });
      expect(ctl.state().mode).toBe("orbit");
      pointer(canvas, "pointerup", { x: 400, y: 300, button: 0 });
      platform.mockReturnValue("MacIntel");
      pointer(canvas, "pointerdown", { x: 400, y: 300, button: 0, buttons: 1, ctrl: true, meta: true });
      expect(ctl.state().mode).toBe("orbit");
      pointer(canvas, "pointerup", { x: 400, y: 300, button: 0 });
      const at: [number, number] = [520, 300];
      const grabbed = hitAt(cam, ...at)!;
      pointer(canvas, "pointerdown", { x: at[0], y: at[1], button: 0, buttons: 1, ctrl: true });
      expect(ctl.state().mode).toBe("pan");
      // Control let go, Shift pressed, Shift let go: still the pan, the grabbed point under the cursor.
      for (const [x, y, sh] of [
        [500, 320, false],
        [300, 410, true],
        [150, 120, false],
      ] as const) {
        pointer(canvas, "pointermove", { x, y, buttons: 1, shift: sh });
        expect(ctl.state().mode).toBe("pan");
        const s = screenOf(cam, grabbed);
        expect(Math.hypot(s[0] - x, s[1] - y)).toBeLessThan(1e-6);
      }
      pointer(canvas, "pointerup", { x: 150, y: 120, button: 0 });
      expect(ctl.state().mode).toBeNull();
    } finally {
      platform.mockRestore();
    }
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
