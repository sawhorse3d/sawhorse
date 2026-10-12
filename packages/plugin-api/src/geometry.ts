// sawhorse.geometry: small pure helpers for vectors, planes, boxes and transforms, in the
// kernel's conventions (metres, Z up; a transform maps p to linear * p + translation, the
// 3 x 3 linear part stored row by row, exactly as kernel ops take it).

export type Vec3 = [number, number, number];

export interface Plane {
  origin: Vec3;
  /** Unit normal. */
  normal: Vec3;
}

export interface Bounds {
  min: Vec3;
  max: Vec3;
}

export interface Transform {
  linear: [Vec3, Vec3, Vec3];
  translation: Vec3;
}

export const vec3 = {
  add: (a: Vec3, b: Vec3): Vec3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]],
  sub: (a: Vec3, b: Vec3): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]],
  scale: (a: Vec3, s: number): Vec3 => [a[0] * s, a[1] * s, a[2] * s],
  dot: (a: Vec3, b: Vec3): number => a[0] * b[0] + a[1] * b[1] + a[2] * b[2],
  cross: (a: Vec3, b: Vec3): Vec3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]],
  length: (a: Vec3): number => Math.hypot(a[0], a[1], a[2]),
  distance: (a: Vec3, b: Vec3): number => Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]),
  /** The unit vector along `a` ([0, 0, 0] stays [0, 0, 0]). */
  normalize: (a: Vec3): Vec3 => {
    const l = Math.hypot(a[0], a[1], a[2]);
    return l > 0 ? [a[0] / l, a[1] / l, a[2] / l] : [0, 0, 0];
  },
  lerp: (a: Vec3, b: Vec3, t: number): Vec3 => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t],
  equals: (a: Vec3, b: Vec3, tol = 1e-9): boolean => Math.abs(a[0] - b[0]) <= tol && Math.abs(a[1] - b[1]) <= tol && Math.abs(a[2] - b[2]) <= tol,
  /** Some unit vector perpendicular to `a`. */
  perpendicular: (a: Vec3): Vec3 => {
    const n = vec3.normalize(a);
    const helper: Vec3 = Math.abs(n[2]) < 0.9 ? [0, 0, 1] : [1, 0, 0];
    return vec3.normalize(vec3.cross(helper, n));
  },
};

export const plane = {
  fromPointNormal: (origin: Vec3, normal: Vec3): Plane => ({ origin: [...origin], normal: vec3.normalize(normal) }),
  /** Through three points (null when they are in a line). */
  fromPoints: (a: Vec3, b: Vec3, c: Vec3): Plane | null => {
    const n = vec3.cross(vec3.sub(b, a), vec3.sub(c, a));
    return vec3.length(n) < 1e-15 ? null : { origin: [...a], normal: vec3.normalize(n) };
  },
  /** Signed distance of `p` from the plane (positive on the normal's side). */
  distance: (pl: Plane, p: Vec3): number => vec3.dot(vec3.sub(p, pl.origin), pl.normal),
  /** The point of the plane nearest `p`. */
  project: (pl: Plane, p: Vec3): Vec3 => vec3.sub(p, vec3.scale(pl.normal, plane.distance(pl, p))),
  /** Two unit vectors in the plane (u, v) with u x v = normal; u is level when the plane is upright. */
  frame: (pl: Plane): { u: Vec3; v: Vec3 } => {
    const n = pl.normal;
    const u: Vec3 = Math.abs(n[2]) > 0.999999 ? [1, 0, 0] : vec3.normalize(vec3.cross([0, 0, 1], n));
    return { u, v: vec3.cross(n, u) };
  },
  /** Where the line through `p` along `dir` meets the plane (null when parallel). */
  intersectLine: (pl: Plane, p: Vec3, dir: Vec3): Vec3 | null => {
    const d = vec3.dot(dir, pl.normal);
    if (Math.abs(d) < 1e-15) return null;
    return vec3.add(p, vec3.scale(dir, -plane.distance(pl, p) / d));
  },
};

export const bbox = {
  /** A box that holds nothing (adding a point makes it that point). */
  empty: (): Bounds => ({ min: [Infinity, Infinity, Infinity], max: [-Infinity, -Infinity, -Infinity] }),
  isEmpty: (b: Bounds): boolean => b.min[0] > b.max[0] || b.min[1] > b.max[1] || b.min[2] > b.max[2],
  fromPoints: (pts: Vec3[]): Bounds => pts.reduce((b, p) => bbox.addPoint(b, p), bbox.empty()),
  addPoint: (b: Bounds, p: Vec3): Bounds => ({
    min: [Math.min(b.min[0], p[0]), Math.min(b.min[1], p[1]), Math.min(b.min[2], p[2])],
    max: [Math.max(b.max[0], p[0]), Math.max(b.max[1], p[1]), Math.max(b.max[2], p[2])],
  }),
  union: (a: Bounds, b: Bounds): Bounds => bbox.addPoint(bbox.addPoint(a, b.min), b.max),
  size: (b: Bounds): Vec3 => (bbox.isEmpty(b) ? [0, 0, 0] : vec3.sub(b.max, b.min)),
  center: (b: Bounds): Vec3 => vec3.lerp(b.min, b.max, 0.5),
  contains: (b: Bounds, p: Vec3, tol = 0): boolean => [0, 1, 2].every((k) => p[k] >= b.min[k] - tol && p[k] <= b.max[k] + tol),
  /** The eight corners. */
  corners: (b: Bounds): Vec3[] => {
    const out: Vec3[] = [];
    for (const x of [b.min[0], b.max[0]]) for (const y of [b.min[1], b.max[1]]) for (const z of [b.min[2], b.max[2]]) out.push([x, y, z]);
    return out;
  },
};

const I3: [Vec3, Vec3, Vec3] = [
  [1, 0, 0],
  [0, 1, 0],
  [0, 0, 1],
];

function mul3(a: [Vec3, Vec3, Vec3], b: [Vec3, Vec3, Vec3]): [Vec3, Vec3, Vec3] {
  const r = (i: number): Vec3 => [0, 1, 2].map((j) => a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j]) as Vec3;
  return [r(0), r(1), r(2)];
}

function apply3(m: [Vec3, Vec3, Vec3], p: Vec3): Vec3 {
  return [vec3.dot(m[0], p), vec3.dot(m[1], p), vec3.dot(m[2], p)];
}

export const transform = {
  identity: (): Transform => ({ linear: [[...I3[0]], [...I3[1]], [...I3[2]]], translation: [0, 0, 0] }),
  translation: (t: Vec3): Transform => ({ linear: [[...I3[0]], [...I3[1]], [...I3[2]]], translation: [...t] }),
  /** A turn by `degrees` about the line through `point` along `axis` (right hand). */
  rotation: (axis: Vec3, degrees: number, point: Vec3 = [0, 0, 0]): Transform => {
    const [x, y, z] = vec3.normalize(axis);
    const a = (degrees * Math.PI) / 180;
    const c = Math.cos(a);
    const s = Math.sin(a);
    const t = 1 - c;
    const linear: [Vec3, Vec3, Vec3] = [
      [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
      [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
      [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
    ];
    return { linear, translation: vec3.sub(point, apply3(linear, point)) };
  },
  /** A scale by `factors` (one number or three) about `point`. */
  scale: (factors: number | Vec3, point: Vec3 = [0, 0, 0]): Transform => {
    const f: Vec3 = typeof factors === "number" ? [factors, factors, factors] : factors;
    const linear: [Vec3, Vec3, Vec3] = [
      [f[0], 0, 0],
      [0, f[1], 0],
      [0, 0, f[2]],
    ];
    return { linear, translation: vec3.sub(point, apply3(linear, point)) };
  },
  /** `a` after `b`: (a * b)(p) = a(b(p)). */
  compose: (a: Transform, b: Transform): Transform => ({ linear: mul3(a.linear, b.linear), translation: vec3.add(apply3(a.linear, b.translation), a.translation) }),
  apply: (m: Transform, p: Vec3): Vec3 => vec3.add(apply3(m.linear, p), m.translation),
  /** A direction (no translation). */
  applyVector: (m: Transform, v: Vec3): Vec3 => apply3(m.linear, v),
  determinant: (m: Transform): number => vec3.dot(m.linear[0], vec3.cross(m.linear[1], m.linear[2])),
  /** The inverse (null when the transform flattens space). */
  invert: (m: Transform): Transform | null => {
    const [a, b, c] = m.linear;
    const det = transform.determinant(m);
    if (Math.abs(det) < 1e-15) return null;
    const cof = (r: Vec3, s: Vec3): Vec3 => vec3.cross(r, s);
    // Columns of the inverse are the cross products of rows over the determinant.
    const c0 = vec3.scale(cof(b, c), 1 / det);
    const c1 = vec3.scale(cof(c, a), 1 / det);
    const c2 = vec3.scale(cof(a, b), 1 / det);
    const linear: [Vec3, Vec3, Vec3] = [
      [c0[0], c1[0], c2[0]],
      [c0[1], c1[1], c2[1]],
      [c0[2], c1[2], c2[2]],
    ];
    return { linear, translation: vec3.scale(apply3(linear, m.translation), -1) };
  },
};

export const geometry = { vec3, plane, bbox, transform };
