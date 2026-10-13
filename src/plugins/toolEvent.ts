// What a plugin's tool hears about the pointer (lane 17).
//
// A tool event, as the app resolves it, carries the model under the cursor: the inference
// engine's snap (a vertex's position, the solid and face under it, the face's frame) and the
// pick (the entity's ids, an edge's ends). That is the model as `model.read` returns it, so a
// plugin without `model.read` gets none of it: only the pointer (position, buttons, the eye
// ray) and, when it has set inference constraints of its own (`ToolView.inference`), the
// point those constraints give on their own: the eye ray's nearest point on its locked line
// (`start` + `lock`), or where the ray meets its plane. Nothing in that point comes from the
// model: it is worked out from the ray and the plugin's own values, the way the plugin could
// work it out itself.
import type { Data } from "../../packages/plugin-api/src/rpc.ts";
import type { V3 } from "../kernel/types";
import { INFERENCE_STYLES } from "../lib/inferenceStyle";
import type { InferContext, Snap } from "../tools/tool";

export interface Ray {
  origin: V3;
  dir: V3;
}

/** The snap a plugin without `model.read` may see: from its own constraints only. */
export interface OwnSnap {
  point: V3;
  kind: string;
  tip: string;
  surface: null;
  entity: null;
}

const sub = (a: V3, b: V3): V3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const dot = (a: V3, b: V3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const along = (p: V3, d: V3, t: number): V3 => [p[0] + d[0] * t, p[1] + d[1] * t, p[2] + d[2] * t];
const finite = (v: V3) => v.every((x) => Number.isFinite(x));

/**
 * The point a tool's own inference constraints give for an eye ray, or null: on its locked
 * line (`start` and `lock`), the point of the line nearest the ray; else on its `plane`,
 * where the ray meets it in front of the eye. No model is consulted.
 */
export function ownSnap(ray: Ray, inf: InferContext): OwnSnap | null {
  if (inf.start && inf.lock) {
    const p = inf.start;
    const d = inf.lock.dir;
    const r = ray.dir;
    const w = sub(p, ray.origin);
    const a = dot(d, d);
    const b = dot(d, r);
    const c = dot(r, r);
    const den = a * c - b * b;
    if (a > 0 && c > 0 && Math.abs(den) > 1e-12 * a * c) {
      // The line's parameter at its closest approach to the ray (w + s d - t r is
      // perpendicular to both d and r).
      const s = (b * dot(r, w) - c * dot(d, w)) / den;
      const point = along(p, d, s);
      if (finite(point)) return { point, kind: inf.lock.kind, tip: INFERENCE_STYLES[inf.lock.kind]?.label ?? "", surface: null, entity: null };
    }
    return null;
  }
  if (inf.plane) {
    const n = inf.plane.normal;
    const den = dot(ray.dir, n);
    if (Math.abs(den) < 1e-12) return null;
    const t = dot(sub(inf.plane.origin, ray.origin), n) / den;
    if (!(t > 0)) return null;
    const point = along(ray.origin, ray.dir, t);
    return finite(point) ? { point, kind: "free", tip: "", surface: null, entity: null } : null;
  }
  return null;
}

/** The same point as a Snap for the tool's marker (the person sees what the plugin gets). */
export function ownMarkerSnap(ray: Ray, inf: InferContext): Snap | null {
  const s = ownSnap(ray, inf);
  if (!s) return null;
  return { point: s.point, kind: s.kind as Snap["kind"], tip: s.tip, surface: null, entity: null, lines: [] };
}

/**
 * A resolved tool event as a plugin without `model.read` receives it: the pointer and the
 * eye ray, `picked` always null, `snap` null or the point of its own constraints
 * (`ownSnap`). Every other field the app may add later is left out, so nothing new can slip
 * through by default.
 */
export function redactToolEvent(e: unknown, inf: InferContext): Data {
  const o = (typeof e === "object" && e !== null ? e : {}) as Record<string, unknown>;
  const ray = o.ray as Ray | undefined;
  const okRay = !!ray && Array.isArray(ray.origin) && Array.isArray(ray.dir) && ray.origin.length === 3 && ray.dir.length === 3;
  const r: Ray = okRay ? { origin: [...ray!.origin] as V3, dir: [...ray!.dir] as V3 } : { origin: [0, 0, 0], dir: [0, 0, -1] };
  return {
    type: typeof o.type === "string" ? o.type : "move",
    x: typeof o.x === "number" ? o.x : 0,
    y: typeof o.y === "number" ? o.y : 0,
    shift: !!o.shift,
    ctrl: !!o.ctrl,
    alt: !!o.alt,
    ray: r as unknown as Data,
    snap: (okRay ? ownSnap(r, inf) : null) as unknown as Data,
    picked: null,
  };
}
