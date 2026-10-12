// @ts-check
// Shelf pin holes: a tool that drills a column of holes into a face, the way a line-boring
// jig does on a cabinet side.
//
// Hover a face: a column of N holes of diameter D at pitch P previews, going up the face
// from the cursor. Click: each hole is drawn on the face and pushed in (the kernel's own
// draw and push/pull, so the solid stays valid and watertight), all as one batch, so one
// Ctrl+Z takes the column away. Type "count, pitch, diameter" in the Value Control Box
// (8, 32mm, 5mm to start); any of them may be left out ("12", ", 37mm"). Esc clears the
// preview. The settings are kept for next time.
import { geometry, model, storage, ui, units } from "sawhorse";

const { vec3 } = geometry;
const MM = 0.001;
const IN = 0.0254;
/** Sides on the polygon that stands for each hole. */
const SIDES = 16;

/** @type {{ count: number, pitch: number, diameter: number }} */
let settings = { count: 8, pitch: 32 * MM, diameter: 5 * MM };
const saved = /** @type {Partial<typeof settings> | null} */ (await storage.app.get("settings"));
if (saved && typeof saved === "object") settings = { ...settings, ...saved };

/** @typedef {{ solid: number, face: number, point: import("sawhorse").Vec3, normal: import("sawhorse").Vec3 }} Spot */
/** @type {Spot | null} */
let spot = null;
let busyDrilling = false;

/** Usual pin depth: 3/8 in, or 10 mm in metric documents. */
const usualDepth = () => (["mm", "cm", "m"].includes(units.system()) ? 10 * MM : 0.375 * IN);

/** Up the face: world up laid onto the face (world Y for a level face). @param {import("sawhorse").Vec3} n */
function upOn(n) {
  const along = (/** @type {import("sawhorse").Vec3} */ w) => vec3.sub(w, vec3.scale(n, vec3.dot(w, n)));
  const z = along([0, 0, 1]);
  return vec3.normalize(vec3.length(z) > 0.1 ? z : along([0, 1, 0]));
}

/** The hole centres for a spot. @param {Spot} s */
function centres(s) {
  const up = upOn(s.normal);
  return Array.from({ length: settings.count }, (_, i) => vec3.add(s.point, vec3.scale(up, i * settings.pitch)));
}

/** A hole's outline on the face. @param {import("sawhorse").Vec3} c @param {import("sawhorse").Vec3} n */
function circle(c, n) {
  const u = upOn(n);
  const v = vec3.cross(n, u);
  const r = settings.diameter / 2;
  return Array.from({ length: SIDES }, (_, k) => {
    const a = (2 * Math.PI * k) / SIDES;
    return vec3.add(c, vec3.add(vec3.scale(u, r * Math.cos(a)), vec3.scale(v, r * Math.sin(a))));
  });
}

/** A length in the document's units, or in millimetres when those would only round it (5 mm, not "~ 3/16"). @param {number} m */
const len = (m) => {
  const f = units.format(m);
  return f.startsWith("~") ? `${Number((m * 1000).toFixed(1))} mm` : f;
};

const describe = () => `${settings.count} holes, ${len(settings.diameter)} across, ${len(settings.pitch)} apart`;

/** @param {import("sawhorse").Tool} tool @param {boolean} [say] update the status line too */
async function show(tool, say = true) {
  if (!spot) {
    await tool.setView({ overlay: [], labels: [], busy: false, status: say ? `Hover a face to place ${describe()}.` : undefined, value: `${settings.count}, ${len(settings.pitch)}, ${len(settings.diameter)}` });
    return;
  }
  const cs = centres(spot);
  const lift = vec3.scale(spot.normal, 0.0002); // just off the face, so the preview is never hidden in it
  await tool.setView({
    overlay: [
      { kind: "segments", points: [vec3.add(cs[0], lift), vec3.add(cs[cs.length - 1], lift)], color: "accent", dashed: true, width: 1.25 },
      ...cs.map((c) => ({ kind: /** @type {const} */ ("loop"), points: circle(vec3.add(c, lift), spot ? spot.normal : [0, 0, 1]), color: "accent", width: 1.75 })),
    ],
    labels: [{ at: cs[cs.length - 1], text: `${settings.count} x ${len(settings.diameter)} @ ${len(settings.pitch)}` }],
    busy: true,
    status: say ? `Click to drill ${describe()}, ${len(usualDepth())} deep. Type count, pitch, diameter to change them; Esc clears.` : undefined,
  });
}

/** How deep the holes go: the usual depth, but never more than 60% through the part. @param {Spot} s */
async function depthFor(s) {
  const solid = await model.solid(s.solid);
  if (!solid) return usualDepth();
  const size = vec3.sub(solid.max, solid.min);
  const across = Math.abs(s.normal[0]) * size[0] + Math.abs(s.normal[1]) * size[1] + Math.abs(s.normal[2]) * size[2];
  return Math.min(usualDepth(), 0.6 * across);
}

/** @param {import("sawhorse").Tool} tool */
async function drill(tool) {
  const s = spot;
  if (!s || busyDrilling) return;
  const solid = await model.solid(s.solid);
  if (solid && !solid.editable) {
    await ui.notify("That face is in a group or component that is not open: double-click it with Select to edit inside it, then drill.", { kind: "warning" });
    return;
  }
  busyDrilling = true;
  const depth = await depthFor(s);
  const batch = model.batch(`Drill ${settings.count} shelf pin holes`);
  for (const c of centres(s)) {
    const hole = batch.op({ op: "draw", target: { on: "face", object: s.solid, face: s.face }, points: circle(c, s.normal), closed: true });
    batch.op({ op: "push_pull", object: s.solid, face: hole.face, distance: -depth });
  }
  try {
    await batch.commit();
    await ui.notify(`Drilled ${describe()}, ${len(depth)} deep. Ctrl+Z takes the column back.`);
  } catch (e) {
    await ui.notify(`Could not drill there: ${e instanceof Error ? e.message : String(e)}. Keep the whole column on the face.`, { kind: "error" });
  } finally {
    busyDrilling = false;
  }
  // The preview again, leaving the result in the status line.
  await show(tool, false);
}

await ui.registerTool({
  id: "drill",
  onActivate: (tool) => show(tool),
  onDeactivate: () => {
    spot = null;
  },
  onEvent: async (e, tool) => {
    const surface = e.snap?.surface ?? e.picked?.surface ?? null;
    if (e.type === "leave" || !surface || surface.at.kind !== "face") {
      if (spot) {
        spot = null;
        await show(tool);
      }
      return;
    }
    const n = surface.frame.normal;
    const point = e.snap ? e.snap.point : surface.point;
    // On the face's plane exactly.
    const onPlane = vec3.sub(point, vec3.scale(n, vec3.dot(vec3.sub(point, surface.frame.origin), n)));
    spot = { solid: surface.at.solid, face: surface.at.face, point: onPlane, normal: n };
    if (e.type === "click") await drill(tool);
    else await show(tool);
  },
  onValue: async (text, tool) => {
    const parts = text.split(",").map((p) => p.trim());
    const next = { ...settings };
    if (parts[0]) {
      const n = Number(parts[0]);
      if (!Number.isInteger(n) || n < 1 || n > 100) return ui.notify(`"${parts[0]}" is not a count of holes (1 to 100).`, { kind: "error" });
      next.count = n;
    }
    for (const [i, key] of /** @type {const} */ ([[1, "pitch"], [2, "diameter"]])) {
      if (!parts[i]) continue;
      const r = units.read(parts[i]);
      if (!r.ok || r.metres <= 0) return ui.notify(`The ${key}: ${r.ok ? "it must be more than zero" : r.error}`, { kind: "error" });
      next[key] = r.metres;
    }
    if (next.pitch <= next.diameter) return ui.notify("The pitch must be more than the diameter, or the holes run together.", { kind: "error" });
    settings = next;
    await storage.app.set("settings", settings);
    await show(tool);
  },
  onCancel: async (tool) => {
    spot = null;
    await show(tool);
  },
});
