// Friendly names for what the status line talks about. The kernel addresses solids,
// sketches and regions by public ids, which carry slot-reuse bits once anything has been
// deleted (the second object in a reused slot is 4294967297). People never see those:
//
//   solids   "solid 3"   the kernel's display number (Object::number: handed out at
//                         creation, never reused within the document's history);
//   faces    "face 6"    the persistent face id (per solid, from 1, never reused);
//   regions  "region 2"  a display index: the region's position among its sketch's
//                         regions, in id order (stable while that sketch is unchanged);
//   sketches "the ground" or "a sketch 30 in up".
//
// The app keeps the naming source current (setNameSource after every scene); tools and
// the status line call the functions below. `friendlyError` rewrites kernel refusals.
import type { DocState, MeshData } from "../kernel/types";

let state: DocState | null = null;
let mesh: MeshData | null = null;

/** The scene names are resolved against (the app calls this after every edit). */
export function setNameSource(s: DocState | null, m: MeshData | null): void {
  state = s;
  mesh = m;
}

/** "solid 3" (display number), or "a solid" when it is not in the current scene. */
export function solidName(id: number): string {
  const n = state?.solids?.find((s) => s.id === id)?.number;
  return n !== undefined ? `solid ${n}` : "a solid";
}

/** "face 6": persistent face ids are already small, per solid and never reused. */
export function faceName(face: number): string {
  return `face ${face}`;
}

/** "face 6 of solid 3". */
export function faceOf(object: number, face: number): string {
  return `${faceName(face)} of ${solidName(object)}`;
}

/** "region 2": the region's 1-based position among its sketch's regions (id order). */
export function regionName(sketch: number, region: number): string {
  const ids = [...new Set((mesh?.parts ?? []).filter((p) => p.kind === "region" && p.owner === sketch).map((p) => p.index))].sort((a, b) => a - b);
  const k = ids.indexOf(region);
  return k >= 0 ? `region ${k + 1}` : "a region";
}

/** Kernel messages name things by public id ("no object #4294967297"): say it in words. */
export function friendlyError(msg: string): string {
  return msg
    .replace(/\bno (object|solid) #\d+/g, "that solid no longer exists")
    .replace(/\bno sketch #\d+/g, "that sketch no longer exists")
    .replace(/\bno region #\d+ in this sketch/g, "that region no longer exists")
    .replace(/\bno group #\d+/g, "that group no longer exists")
    .replace(/\bno guide or dimension #\d+/g, "that guide or dimension no longer exists")
    .replace(/\bno group or component #\d+/g, "that group or component no longer exists")
    .replace(/\bno component definition #\d+/g, "that component no longer exists")
    .replace(/\bno imported mesh #\d+/g, "that imported mesh no longer exists")
    // Lane 23b: a curve, a section plane, a sheet and its items read as sentences too (the
    // Eraser once said "no curve that one there").
    .replace(/\bno curve #\d+( there)?/g, "that curve no longer exists")
    .replace(/\bno section plane #\d+/g, "that section plane no longer exists")
    .replace(/\ba scene names section plane #\d+, which does not exist/g, "a scene names a section plane that no longer exists")
    .replace(/\bno item #\d+ on sheet #\d+/g, "that item is no longer on that sheet")
    .replace(/\bno item #\d+/g, "that item no longer exists")
    .replace(/\bno sheet #\d+/g, "that sheet no longer exists")
    .replace(/\bno region #\d+/g, "that region no longer exists")
    .replace(/\b(object|solid) #(\d+)/g, (_m, _w: string, id: string) => solidName(Number(id)))
    .replace(/\bface #(\d+)/g, (_m, id: string) => faceName(Number(id)))
    .replace(/\bregion #(\d+)/g, "that region")
    .replace(/\bsketch #(\d+)/g, "that sketch")
    .replace(/\bgroup #(\d+)/g, "that group")
    .replace(/\bguide or dimension #(\d+)/g, "that guide or dimension")
    .replace(/\bcurve #(\d+)/g, "that curve")
    .replace(/\bsection plane #(\d+)/g, "that section plane")
    .replace(/\bsheet #(\d+)/g, "that sheet")
    // Anything else named by id: "no <thing> #N" is a thing that is gone.
    .replace(/\bno ([a-z]+(?: [a-z]+)?) #\d+/g, "that $1 no longer exists")
    .replace(/#(\d+)/g, "that one")
    // A bare public id (slot-reuse bits make them ten digits or more) is never shown.
    .replace(/\b\d{10,}\b/g, "that one");
}

/** True when a status text names something by a raw id (tests use it as a tripwire). */
export function hasRawId(text: string): boolean {
  return /#\d/.test(text) || /\b\d{10,}\b/.test(text);
}
