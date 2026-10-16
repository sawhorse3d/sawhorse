// Lane 21 (issue #5): the camera controller's maths. Orbit about a picked pivot keeps the
// pivot exactly where it was on screen, turns about world +Z and never flips the view over
// the pole; pan keeps the grabbed point under the cursor; zoom keeps the cursor's point.
import * as THREE from "three";
import { describe, expect, it } from "vitest";
import type { Vec3 } from "./camera";
import {
  ELEVATION_LIMIT,
  add,
  cross,
  depthOf,
  dot,
  dragAngles,
  elevation,
  elevationStep,
  length,
  lookTarget,
  normalize,
  orbit,
  panOffset,
  projectNdc,
  rayAt,
  rotate,
  sub,
  wheelZoomFactor,
  zoomAt,
  type Lens,
  type Pose,
} from "./navmath";

/** A camera at `eye` looking at `at`, Z up (no roll). */
function poseLooking(eye: Vec3, at: Vec3, upHint: Vec3 = [0, 0, 1]): Pose {
  const fwd = normalize(sub(at, eye));
  const right = normalize(cross(fwd, upHint));
  const up = cross(right, fwd);
  return { eye, right, up, fwd };
}

const PERSP: Lens = { ortho: false, fovDeg: 35, halfHeight: 0, zoom: 1, aspect: 16 / 9 };
const PARALLEL: Lens = { ortho: true, fovDeg: 35, halfHeight: 0.4, zoom: 2.5, aspect: 1.6 };

/** A small deterministic generator (mulberry32), so a failure names its case. */
function rng(seed: number) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const ndcDistance = (a: [number, number], b: [number, number]) => Math.hypot(a[0] - b[0], a[1] - b[1]);

function expectOrthonormal(p: Pose) {
  for (const v of [p.right, p.up, p.fwd]) expect(Math.abs(length(v) - 1)).toBeLessThan(1e-12);
  expect(Math.abs(dot(p.right, p.up))).toBeLessThan(1e-12);
  expect(Math.abs(dot(p.right, p.fwd))).toBeLessThan(1e-12);
  expect(Math.abs(dot(p.up, p.fwd))).toBeLessThan(1e-12);
  // right x up = -fwd (a camera looking down its -Z axis).
  const c = cross(p.right, p.up);
  expect(length(add(c, p.fwd))).toBeLessThan(1e-12);
}

describe("orbit about a picked pivot", () => {
  it("keeps the pivot's projection identical (to 1e-9 in NDC) for any angle, in both projections", () => {
    const r = rng(2026_10_16);
    let worst = 0;
    for (let i = 0; i < 2000; i++) {
      const lens = i % 2 ? PERSP : PARALLEL;
      // A corner of a box somewhere near the origin, zoomed in on from some way off.
      const pivot: Vec3 = [r() * 0.6 - 0.1, r() * 0.4 - 0.1, r() * 0.3];
      const look: Vec3 = add(pivot, [(r() - 0.5) * 0.05, (r() - 0.5) * 0.05, (r() - 0.5) * 0.05]);
      const eye: Vec3 = add(look, [(r() - 0.5) * 4, (r() - 0.5) * 4, 0.2 + r() * 2]);
      const pose = poseLooking(eye, look);
      const before = projectNdc(pose, lens, pivot);
      // Any angle: several turns of azimuth, and elevation steps well past the limit.
      const az = (r() - 0.5) * 8 * Math.PI;
      const el = (r() - 0.5) * 4 * Math.PI;
      const after = orbit(pose, pivot, az, el);
      expectOrthonormal(after.pose);
      const d = ndcDistance(before, projectNdc(after.pose, lens, pivot));
      worst = Math.max(worst, d);
      expect(d, `case ${i}: az ${az}, el ${el}`).toBeLessThan(1e-9);
    }
    // The figure the lane reports.
    console.info(`[navmath] pivot pinned: worst NDC drift over 2000 random orbits = ${worst.toExponential(2)}`);
  });

  it("stays pinned over a long drag made of many small steps (no drift)", () => {
    const pivot: Vec3 = [0.3048, 0.2032, 0.3048];
    let pose = poseLooking([-0.5, -0.9, 0.9], [0.31, 0.21, 0.29]);
    const before = projectNdc(pose, PERSP, pivot);
    for (let i = 0; i < 4000; i++) {
      const a = dragAngles(i % 7 === 0 ? -3 : 2, i % 5 === 0 ? 9 : -1, 720);
      pose = orbit(pose, pivot, a.azimuth, a.elevation).pose;
    }
    expect(ndcDistance(before, projectNdc(pose, PERSP, pivot))).toBeLessThan(1e-9);
    expectOrthonormal(pose);
  });

  it("turns about world +Z: the eye keeps its height and its distance from the pivot's vertical", () => {
    const pivot: Vec3 = [0.2, 0.1, 0.15];
    const pose = poseLooking([1.2, -1.4, 0.9], [0.25, 0.15, 0.1]);
    for (const az of [0.1, 1, Math.PI / 2, 3, -2.5]) {
      const { pose: p } = orbit(pose, pivot, az, 0);
      expect(p.eye[2]).toBeCloseTo(pose.eye[2], 12);
      const h0 = sub(pose.eye, pivot);
      const h1 = sub(p.eye, pivot);
      expect(Math.hypot(h1[0], h1[1])).toBeCloseTo(Math.hypot(h0[0], h0[1]), 12);
      // The turn is exactly `az` counter-clockwise seen from above.
      const turned = Math.atan2(h0[0] * h1[1] - h0[1] * h1[0], h0[0] * h1[0] + h0[1] * h1[1]);
      expect(turned).toBeCloseTo(Math.atan2(Math.sin(az), Math.cos(az)), 12);
      // The view direction turns with it and keeps its elevation.
      expect(elevation(p.fwd)).toBeCloseTo(elevation(pose.fwd), 12);
      expect(length(sub(p.fwd, rotate(pose.fwd, [0, 0, 1], az)))).toBeLessThan(1e-12);
      // No roll: the right vector stays level.
      expect(Math.abs(p.right[2])).toBeLessThan(1e-12);
    }
  });

  it("dragging right turns the model right; dragging down raises the eye", () => {
    const pivot: Vec3 = [0, 0, 0];
    const pose = poseLooking([0, -2, 1], pivot);
    const right = dragAngles(100, 0, 600);
    // The model turns right: a point in front of the pivot (toward the eye) moves right.
    const front: Vec3 = [0, -0.3, 0];
    const p0 = projectNdc(pose, PERSP, front);
    const p1 = projectNdc(orbit(pose, pivot, right.azimuth, right.elevation).pose, PERSP, front);
    expect(p1[0]).toBeGreaterThan(p0[0]);
    const down = dragAngles(0, 100, 600);
    const raised = orbit(pose, pivot, down.azimuth, down.elevation).pose;
    expect(raised.eye[2]).toBeGreaterThan(pose.eye[2]);
    expect(elevation(raised.fwd)).toBeGreaterThan(elevation(pose.fwd));
  });

  it("never flips the up vector: the elevation stops 0.5 degrees short of the poles", () => {
    const pivot: Vec3 = [0.1, 0.2, 0];
    for (const dir of [1, -1]) {
      let pose = poseLooking([1, -1, 0.8], pivot);
      // 2000 px of drag in 10 px steps on a 600 px view: ~21 turns' worth of elevation.
      for (let i = 0; i < 200; i++) {
        const a = dragAngles(0, 10 * dir, 600);
        pose = orbit(pose, pivot, a.azimuth, a.elevation).pose;
        expect(pose.up[2]).toBeGreaterThan(0);
        expect(Math.abs(elevation(pose.fwd))).toBeLessThanOrEqual(ELEVATION_LIMIT + 1e-12);
        expect(Math.abs(pose.right[2])).toBeLessThan(1e-12);
      }
      expect(Math.abs(elevation(pose.fwd))).toBeCloseTo(ELEVATION_LIMIT, 12);
      // ... and the azimuth still turns the view there (no lock).
      const a = dragAngles(150, 0, 600);
      const turned = orbit(pose, pivot, a.azimuth, a.elevation).pose;
      expect(dot(turned.right, pose.right)).toBeLessThan(Math.cos(1));
      // Coming back responds at once (the overshoot is not banked).
      const back = dragAngles(0, -10 * dir, 600);
      const b = orbit(turned, pivot, back.azimuth, back.elevation);
      expect(Math.abs(b.elevation)).toBeCloseTo((2 * Math.PI * 10) / 600, 12);
    }
  });

  it("a view beyond the limit (Top looks straight down) comes back without a jump", () => {
    expect(elevationStep(Math.PI / 2, 0.1)).toBe(0);
    expect(elevationStep(Math.PI / 2, -0.1)).toBeCloseTo(-0.1, 15);
    expect(elevationStep(ELEVATION_LIMIT - 0.01, 0.5)).toBeCloseTo(0.01, 15);
    expect(elevationStep(-Math.PI / 2, -0.2)).toBe(0);
    // From the Top view (up = +Y), orbiting both ways keeps the pivot and lands unrolled.
    const pivot: Vec3 = [0.3, 0.2, 0];
    const top: Pose = { eye: [0.3, 0.2, 5], right: [1, 0, 0], up: [0, 1, 0], fwd: [0, 0, -1] };
    const before = projectNdc(top, PARALLEL, pivot);
    const p = orbit(top, pivot, 0.4, -0.3).pose;
    expect(ndcDistance(before, projectNdc(p, PARALLEL, pivot))).toBeLessThan(1e-9);
    expect(Math.abs(p.right[2])).toBeLessThan(1e-12);
    expect(p.up[2]).toBeGreaterThan(0);
  });
});

describe("pan keeps the grabbed point under the cursor", () => {
  it("in both projections, at the point's own depth", () => {
    const r = rng(42);
    for (let i = 0; i < 500; i++) {
      const lens = i % 2 ? PERSP : PARALLEL;
      const pose = poseLooking([(r() - 0.5) * 4, -2 - r() * 2, 0.5 + r() * 2], [r() * 0.5, r() * 0.5, r() * 0.2]);
      // The point grabbed at the press: something under some pixel.
      const grab = rayAt(pose, lens, (r() - 0.5) * 1.6, (r() - 0.5) * 1.6);
      const anchor = add(grab.origin, [grab.dir[0] * (1 + r() * 3), grab.dir[1] * (1 + r() * 3), grab.dir[2] * (1 + r() * 3)]);
      const depth = depthOf(pose, anchor);
      // The cursor now.
      const c: [number, number] = [(r() - 0.5) * 1.9, (r() - 0.5) * 1.9];
      const off = panOffset(pose.fwd, anchor, rayAt(pose, lens, c[0], c[1]));
      const moved: Pose = { ...pose, eye: add(pose.eye, off) };
      expect(ndcDistance(projectNdc(moved, lens, anchor), c), `case ${i}`).toBeLessThan(1e-9);
      expect(depthOf(moved, anchor)).toBeCloseTo(depth, 10);
      // The move is square to the view: the view direction is unchanged, the eye stays in its plane.
      expect(Math.abs(dot(off, pose.fwd))).toBeLessThan(1e-12);
    }
  });
});

describe("zoom keeps the cursor's point fixed", () => {
  it("perspective: the eye moves along the cursor ray toward what is under it", () => {
    const pose = poseLooking([-1, -2, 1.5], [0.3, 0.2, 0.1]);
    const c: [number, number] = [0.4, -0.3];
    const ray = rayAt(pose, PERSP, c[0], c[1]);
    const anchor = add(ray.origin, [ray.dir[0] * 2.2, ray.dir[1] * 2.2, ray.dir[2] * 2.2]);
    let p = pose;
    for (const f of [wheelZoomFactor(-400), wheelZoomFactor(-120), wheelZoomFactor(600), 3, 0.2]) {
      const before = length(sub(p.eye, anchor));
      const z = zoomAt(p, PERSP, c[0], c[1], f, anchor);
      p = z.pose;
      expect(ndcDistance(projectNdc(p, PERSP, anchor), c)).toBeLessThan(1e-9);
      expect(length(sub(p.eye, anchor))).toBeCloseTo(before / f, 12);
      expect(z.zoom).toBe(1);
    }
  });

  it("parallel: the zoom scales and the point under the cursor stays put", () => {
    const pose = poseLooking([-3, -3, 3], [0.2, 0.1, 0.1]);
    const c: [number, number] = [-0.55, 0.62];
    const ray = rayAt(pose, PARALLEL, c[0], c[1]);
    // Any point along the cursor ray (depth does not matter in parallel).
    const points = [0.5, 3, 9].map((t) => add(ray.origin, [ray.dir[0] * t, ray.dir[1] * t, ray.dir[2] * t]));
    let p = pose;
    let lens = PARALLEL;
    for (const f of [wheelZoomFactor(-400), 0.5, 7]) {
      const z = zoomAt(p, lens, c[0], c[1], f, points[0]);
      expect(z.zoom).toBeCloseTo(lens.zoom * f, 12);
      p = z.pose;
      lens = { ...lens, zoom: z.zoom };
      for (const q of points) expect(ndcDistance(projectNdc(p, lens, q), c)).toBeLessThan(1e-9);
    }
  });

  it("wheel steps match the ones three.js OrbitControls took (5% per 100 px)", () => {
    expect(wheelZoomFactor(-400)).toBeCloseTo(1 / Math.pow(0.95, 4), 14);
    expect(wheelZoomFactor(600)).toBeCloseTo(Math.pow(0.95, 6), 14);
    expect(wheelZoomFactor(-3, 1)).toBeCloseTo(1 / Math.pow(0.95, 0.48), 14);
    expect(wheelZoomFactor(1, 2)).toBeCloseTo(Math.pow(0.95, 1), 14);
    expect(wheelZoomFactor(-10, 0, true)).toBeCloseTo(1 / Math.pow(0.95, 1), 14);
    expect(wheelZoomFactor(0)).toBe(1);
  });
});

describe("the look-at point", () => {
  it("lies on the view direction at the pivot's depth", () => {
    const pose = poseLooking([-1, -2, 1.5], [0.3, 0.2, 0.1]);
    const pivot: Vec3 = [0.5, 0.1, 0];
    const t = lookTarget(pose, pivot, 1);
    expect(depthOf(pose, t)).toBeCloseTo(depthOf(pose, pivot), 12);
    expect(length(cross(normalize(sub(t, pose.eye)), pose.fwd))).toBeLessThan(1e-12);
    // A pivot behind the eye falls back to the given depth.
    const behind = add(pose.eye, [-pose.fwd[0], -pose.fwd[1], -pose.fwd[2]]);
    expect(depthOf(pose, lookTarget(pose, behind, 2))).toBeCloseTo(2, 12);
  });
});

describe("the maths matches the renderer's cameras", () => {
  const toThree = (pose: Pose, cam: THREE.Camera) => {
    cam.position.set(...pose.eye);
    const m = new THREE.Matrix4().makeBasis(new THREE.Vector3(...pose.right), new THREE.Vector3(...pose.up), new THREE.Vector3(...pose.fwd).negate());
    cam.quaternion.setFromRotationMatrix(m);
    cam.updateMatrixWorld(true);
  };
  it("projects and casts rays as three.js does, perspective and parallel", () => {
    const pose = poseLooking([-1.2, -2.1, 1.4], [0.3, 0.2, 0.1]);
    const persp = new THREE.PerspectiveCamera(PERSP.fovDeg, PERSP.aspect, 0.01, 100);
    persp.zoom = 1.7;
    persp.updateProjectionMatrix();
    const H = PARALLEL.halfHeight;
    const ortho = new THREE.OrthographicCamera(-H * PARALLEL.aspect, H * PARALLEL.aspect, H, -H, -100, 100);
    ortho.zoom = PARALLEL.zoom;
    ortho.updateProjectionMatrix();
    toThree(pose, persp);
    toThree(pose, ortho);
    const cases: [THREE.Camera, Lens][] = [
      [persp, { ...PERSP, zoom: 1.7 }],
      [ortho, PARALLEL],
    ];
    for (const [cam, lens] of cases) {
      for (const p of [[0.1, 0.2, 0.3], [0.5, -0.1, 0], [-0.2, 0.4, 0.25]] as Vec3[]) {
        const want = new THREE.Vector3(...p).project(cam);
        const got = projectNdc(pose, lens, p);
        expect(Math.abs(got[0] - want.x)).toBeLessThan(1e-9);
        expect(Math.abs(got[1] - want.y)).toBeLessThan(1e-9);
      }
      const rc = new THREE.Raycaster();
      rc.setFromCamera(new THREE.Vector2(0.3, -0.4), cam);
      const mine = rayAt(pose, lens, 0.3, -0.4);
      // Same line: the directions agree and three's origin lies on mine.
      expect(length(sub(mine.dir, rc.ray.direction.toArray() as Vec3))).toBeLessThan(1e-9);
      const o = sub(rc.ray.origin.toArray() as Vec3, mine.origin);
      expect(length(cross(o, mine.dir))).toBeLessThan(1e-9);
    }
  });
});
