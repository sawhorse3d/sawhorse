// Which target a finished shape goes to. A shape started on a solid's face is drawn on
// that face (cuts where it crosses the face's edges, an imprint when it closes inside)
// when it stays on that solid: every click on the solid, or the first one inside the
// face. A shape that leaves the solid (a top board drawn from one side's corner to the
// other side's) becomes a new sketch on that plane instead, which is what the user is
// building: a new part.
import type { Entity, V3 } from "../kernel/types";
import { onPlane, planeAt, type DrawPlane } from "./draw";
import type { Snap } from "./tool";

function ownerObject(e: Entity | null): number | null {
  if (!e) return null;
  if (e.kind === "face" || e.kind === "object") return e.object;
  if ((e.kind === "edge" || e.kind === "vertex") && e.owner.kind === "object") return e.owner.id;
  return null;
}

/** The solid the snapped point lies on (on the drawing plane), if any. */
export function snapObject(s: Snap, plane: DrawPlane): number | null {
  if (!onPlane(plane.frame, s.point)) return null;
  const fromEntity = ownerObject(s.entity);
  if (fromEntity !== null) return fromEntity;
  for (const sf of [s.surface, s.adjacent]) if (sf?.at.kind === "face" && onPlane(sf.frame, s.point)) return sf.at.object;
  return null;
}

/** The plane as a sketch plane (a new sketch, or the one already on it). */
export function asSketchPlane(plane: DrawPlane): DrawPlane {
  const f = plane.frame;
  const p = planeAt(f.origin, f.normal);
  return { ...p, frame: f };
}

/** Resolve the target for a shape whose clicks were `snaps` (first click first). */
export function finalTarget(plane: DrawPlane, snaps: Snap[]): DrawPlane {
  if (plane.target.on !== "face") return plane;
  const obj = plane.target.object;
  if (snaps.every((s) => snapObject(s, plane) === obj)) return plane;
  if (snaps[0]?.kind === "on-face") return plane;
  return asSketchPlane(plane);
}

/** Points equal within a nanometre. */
export function same(a: V3, b: V3): boolean {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]) <= 1e-9;
}
