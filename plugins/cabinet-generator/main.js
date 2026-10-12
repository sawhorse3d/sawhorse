// @ts-check
// Cabinet generator: a frameless (European-style) cabinet from its sizes.
//
// Every part is a component of its own, named the way a cut list reads it: Side (the Left
// side and the Right side are its two copies), Top, Bottom, Shelf (Shelf 1, Shelf 2 ...),
// Back and Toe kick, all inside one "Cabinet" component. The whole cabinet is one batch,
// so one Ctrl+Z takes it away. Generating again while the cabinet this plugin made is
// selected replaces it in place; otherwise a new cabinet goes beside the model.
//
// Sizes are metres inside; the panel shows and reads them in the document's units.
import { events, model, storage, ui, units } from "sawhorse";

const IN = 0.0254;
const MM = 0.001;

/** @typedef {{ width: number, height: number, depth: number, thickness: number, shelves: number, back: boolean, toeKick: number }} Sizes */

/** Sensible first sizes in the document's units (a base cabinet). @returns {Sizes} */
function defaults() {
  const metric = ["mm", "cm", "m"].includes(units.system());
  return metric
    ? { width: 600 * MM, height: 720 * MM, depth: 560 * MM, thickness: 18 * MM, shelves: 1, back: true, toeKick: 100 * MM }
    : { width: 24 * IN, height: 34.5 * IN, depth: 24 * IN, thickness: 0.75 * IN, shelves: 1, back: true, toeKick: 4 * IN };
}

/** The back panel is thin sheet stock, a quarter inch or 6 mm. */
const backThickness = () => (["mm", "cm", "m"].includes(units.system()) ? 6 * MM : 0.25 * IN);
/** How far the toe kick board sits back from the front. */
const toeSetback = () => (["mm", "cm", "m"].includes(units.system()) ? 75 * MM : 3 * IN);

/** @param {Sizes} s @returns {string | null} what is wrong, or null */
function problem(s) {
  const f = units.format;
  if (!(s.thickness > 0)) return "The stock needs a thickness.";
  if (!(s.width > 2 * s.thickness)) return `The width must be more than two thicknesses (${f(2 * s.thickness)}).`;
  if (!(s.depth > s.thickness + (s.back ? backThickness() : 0))) return "The cabinet is not deep enough for its back.";
  if (!(s.toeKick >= 0)) return "The toe kick cannot be negative.";
  if (!Number.isInteger(s.shelves) || s.shelves < 0 || s.shelves > 20) return "Shelves: a whole number from 0 to 20.";
  const inside = s.height - s.toeKick - 2 * s.thickness - s.shelves * s.thickness;
  if (!(inside > 0)) return `It is not tall enough for a top, a bottom${s.shelves ? ` and ${s.shelves} ${s.shelves === 1 ? "shelf" : "shelves"}` : ""} over a ${f(s.toeKick)} toe kick.`;
  return null;
}

/**
 * The parts, each with its size (in its own axes) and where its copies go.
 * x runs across the front, y from the front to the back, z up.
 * @param {Sizes} s
 */
export function parts(s) {
  const t = s.thickness;
  const b = s.back ? backThickness() : 0;
  const innerW = s.width - 2 * t;
  const innerD = s.depth - b;
  /** @type {import("sawhorse").PartSpec[]} */
  const list = [
    // The left side is the right side mirrored, as in the shop: its inside face is the same
    // face of the part, so holes drilled into one side's inside go into both insides.
    {
      name: "Side",
      boxes: [{ min: [0, 0, 0], size: [t, s.depth, s.height] }],
      copies: [
        { at: { linear: [[-1, 0, 0], [0, 1, 0], [0, 0, 1]], translation: [t, 0, 0] }, name: "Left side" },
        { at: [s.width - t, 0, 0], name: "Right side" },
      ],
    },
    { name: "Top", boxes: [{ min: [0, 0, 0], size: [innerW, innerD, t] }], copies: [{ at: [t, 0, s.height - t], name: "Top" }] },
    { name: "Bottom", boxes: [{ min: [0, 0, 0], size: [innerW, innerD, t] }], copies: [{ at: [t, 0, s.toeKick], name: "Bottom" }] },
  ];
  if (s.shelves > 0) {
    // Equal openings between the bottom, the shelves and the top.
    const floor = s.toeKick + t;
    const gap = (s.height - t - floor - s.shelves * t) / (s.shelves + 1);
    list.push({
      name: "Shelf",
      boxes: [{ min: [0, 0, 0], size: [innerW, innerD, t] }],
      copies: Array.from({ length: s.shelves }, (_, i) => ({ at: /** @type {import("sawhorse").Vec3} */ ([t, 0, floor + gap * (i + 1) + t * i]), name: `Shelf ${i + 1}` })),
    });
  }
  if (s.back) list.push({ name: "Back", boxes: [{ min: [0, 0, 0], size: [innerW, b, s.height - s.toeKick] }], copies: [{ at: [t, s.depth - b, s.toeKick], name: "Back" }] });
  if (s.toeKick > 0) list.push({ name: "Toe kick", boxes: [{ min: [0, 0, 0], size: [innerW, t, s.toeKick] }], copies: [{ at: [t, toeSetback(), 0], name: "Toe kick" }] });
  return list;
}

/** @type {Sizes} */
let sizes = defaults();
const saved = /** @type {Partial<Sizes> | null} */ (await storage.app.get("sizes"));
if (saved && typeof saved === "object") sizes = { ...sizes, ...saved };

/** The cabinet this plugin made last in this document, if it is the selection. */
async function selectedOwnCabinet() {
  const mine = /** @type {number | null} */ (await storage.document.get("cabinet"));
  if (mine === null) return null;
  const sel = await model.selection();
  if (sel.length !== 1 || sel[0].kind !== "instance" || sel[0].id !== mine) return null;
  return (await model.nodes()).find((n) => n.id === mine) ?? null;
}

const panel = await ui.registerPanel({
  id: "cabinet",
  title: "Cabinet",
  body: [
    { type: "number", id: "width", label: "Width", unit: "length", value: sizes.width, min: 0 },
    { type: "number", id: "height", label: "Height", unit: "length", value: sizes.height, min: 0 },
    { type: "number", id: "depth", label: "Depth", unit: "length", value: sizes.depth, min: 0 },
    { type: "number", id: "thickness", label: "Stock", unit: "length", value: sizes.thickness, min: 0 },
    { type: "number", id: "shelves", label: "Shelves", unit: "count", value: sizes.shelves, min: 0, max: 20 },
    { type: "checkbox", id: "back", label: "Back panel", value: sizes.back },
    { type: "number", id: "toeKick", label: "Toe kick", unit: "length", value: sizes.toeKick, min: 0 },
    { type: "button", id: "generate", label: "Generate cabinet", primary: true },
    { type: "note", id: "status", text: "" },
  ],
  onChange: (_id, values) => {
    sizes = { ...sizes, .../** @type {Partial<Sizes>} */ (values) };
    void storage.app.set("sizes", sizes);
    return explain();
  },
  onAction: (id, values) => {
    if (id !== "generate") return;
    sizes = { ...sizes, .../** @type {Partial<Sizes>} */ (values) };
    return generate();
  },
});

/** Keep the note and the button saying what Generate will do. */
async function explain() {
  const wrong = problem(sizes);
  const own = await selectedOwnCabinet();
  const f = units.format;
  const what = `${f(sizes.width)} x ${f(sizes.height)} x ${f(sizes.depth)}, ${sizes.shelves} ${sizes.shelves === 1 ? "shelf" : "shelves"}${sizes.back ? ", back" : ""}${sizes.toeKick > 0 ? `, ${f(sizes.toeKick)} toe kick` : ""}`;
  await panel.update({
    fields: {
      generate: { label: own ? "Update selected cabinet" : "Generate cabinet", disabled: !!wrong },
      status: wrong ? { text: wrong, tone: "warning" } : { text: own ? `Replaces the selected cabinet with ${what}.` : `Adds a cabinet: ${what}.`, tone: "info" },
    },
  });
}

async function generate() {
  const wrong = problem(sizes);
  if (wrong) {
    await ui.notify(wrong, { kind: "warning" });
    return;
  }
  const own = await selectedOwnCabinet();
  const f = units.format;
  const label = `Cabinet ${f(sizes.width)} x ${f(sizes.height)} x ${f(sizes.depth)}`;
  const batch = model.batch(own ? "Update cabinet" : "Generate cabinet");
  /** @type {import("sawhorse").Vec3 | import("sawhorse").Transform} */
  let at = [0, 0, 0];
  if (own) {
    // Same place as the one it replaces.
    at = own.transform;
    batch.delete({ node: own.id });
  } else {
    const b = await model.bounds();
    if (b) at = [b.max[0] + (["mm", "cm", "m"].includes(units.system()) ? 50 * MM : 2 * IN), b.min[1], 0];
  }
  const cabinet = batch.component({ name: label, instanceName: "Cabinet", parts: parts(sizes), at });
  try {
    const result = await batch.commit();
    const id = /** @type {number} */ (result.value(cabinet.node));
    await storage.document.set("cabinet", id);
    await model.select([{ kind: "instance", id }]);
    await ui.notify(`${own ? "Updated the cabinet" : "Made a cabinet"}: ${parts(sizes).map((p) => (p.copies.length > 1 ? `${p.name} x ${p.copies.length}` : p.name)).join(", ")}. The cut list lists every part.`);
  } catch (e) {
    await ui.notify(`Could not make the cabinet: ${e instanceof Error ? e.message : String(e)}`, { kind: "error" });
  }
  await explain();
}

await ui.registerCommand({ id: "generate", run: generate });
events.on("selectionChanged", () => explain());
events.on("documentOpened", () => explain());
await explain();
