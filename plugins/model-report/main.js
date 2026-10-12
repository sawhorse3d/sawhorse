// @ts-check
// Model report: what is in the model, kept up to date as it changes.
//
// Counts (solids, groups, components, instances, faces, edges), the overall size, the total
// volume, board feet by material and sheet goods from the cut list, and warnings (groups
// with nothing solid in them, parts too thin to cut, imported meshes that are not solid).
// "Export report" saves it as Markdown and as CSV through the app's save dialog. The units
// it reports in (the document's, inches or millimetres) are remembered.
import { events, file, model, storage, ui, units } from "sawhorse";

const IN = 0.0254;
/** Parts thinner than this are flagged. */
const THIN = 0.0005;

/** @type {"document" | "in" | "mm"} */
let unitPref = /** @type {any} */ ((await storage.app.get("units")) ?? "document");
if (!["document", "in", "mm"].includes(unitPref)) unitPref = "document";

const metric = () => (unitPref === "document" ? ["mm", "cm", "m"].includes(units.system()) : unitPref === "mm");
const num = (/** @type {number} */ x, /** @type {number} */ places) => new Intl.NumberFormat("en-US", { maximumFractionDigits: places, minimumFractionDigits: 0 }).format(x);
/** @param {number} m */
function len(m) {
  if (unitPref === "document") return units.format(m);
  return unitPref === "mm" ? `${num(m * 1000, 1)} mm` : `${num(m / IN, 3)} in`;
}
/** @param {number} m3 */
const vol = (m3) => (metric() ? `${num(m3 * 1e6, 1)} cm³` : `${num(m3 / IN ** 3, 2)} in³`);
/** @param {number} m2 */
const area = (m2) => (metric() ? `${num(m2 * 1e4, 1)} cm²` : `${num(m2 / IN ** 2, 1)} sq in`);

/** Everything the report says, worked out from the model. */
export async function gather() {
  const [doc, stats, bounds, cut, nodes, tree] = await Promise.all([model.document(), model.stats(), model.bounds(), model.cutList(), model.nodes(), model.tree()]);
  /** @type {string[]} */
  const warnings = [];
  /** @param {import("sawhorse").TreeNode} t @returns {{ solids: number, meshes: number }} */
  const inside = (t) => t.children.reduce((a, c) => {
    const k = c.kind === "solid" ? { solids: 1, meshes: 0 } : c.kind === "mesh" ? { solids: 0, meshes: 1 } : inside(c);
    return { solids: a.solids + k.solids, meshes: a.meshes + k.meshes };
  }, { solids: 0, meshes: 0 });
  /** @param {import("sawhorse").TreeNode} t */
  const walk = (t) => {
    if (t.kind === "group") {
      const k = inside(t);
      if (k.solids === 0) warnings.push(`Group "${t.label}" holds no solid.`);
      else if (k.meshes > 0) warnings.push(`Group "${t.label}" holds ${k.meshes} mesh${k.meshes === 1 ? "" : "es"} that ${k.meshes === 1 ? "is" : "are"} not solid.`);
    }
    t.children.forEach(walk);
  };
  walk(tree);
  for (const r of cut.rows) if (r.thickness < THIN) warnings.push(`"${r.name}" is ${len(r.thickness)} thick: too thin to cut.`);
  if (stats.meshes > 0) warnings.push(`${stats.meshes} imported mesh${stats.meshes === 1 ? " is" : "es are"} not solid (Edit > Delete Imported Meshes).`);
  if (!stats.watertight) warnings.push("Some geometry is open (not watertight).");
  void nodes;
  return { doc, stats, bounds, cut, warnings };
}

/** @param {Awaited<ReturnType<typeof gather>>} r */
function countRows(r) {
  const s = r.stats;
  return [
    ["Solids", s.solids],
    ["Groups", s.groups],
    ["Components", s.components],
    ["Instances", s.instances],
    ["Faces", s.faces],
    ["Edges", s.edges],
  ];
}

/** @param {Awaited<ReturnType<typeof gather>>} r */
function materialRows(r) {
  /** @type {(string | number)[][]} */
  const rows = r.cut.totals.lumber.map((l) => [l.material ?? "(no material)", `${num(l.boardFeet, 2)} bd ft`]);
  for (const s of r.cut.totals.sheets) rows.push([`${s.material ?? "Sheet"} ${len(s.thickness)}`, `${s.sheets} sheet${s.sheets === 1 ? "" : "s"} (${area(s.area)})`]);
  if (!rows.length) rows.push(["(no parts)", ""]);
  return rows;
}

/** @param {Awaited<ReturnType<typeof gather>>} r */
function sizeText(r) {
  if (!r.bounds) return "Empty model.";
  const d = [0, 1, 2].map((k) => /** @type {any} */ (r.bounds).max[k] - /** @type {any} */ (r.bounds).min[k]);
  return `Overall ${len(d[0])} wide x ${len(d[1])} deep x ${len(d[2])} high; volume ${vol(r.stats.volume)}; ${num(r.cut.totals.boardFeet, 2)} board feet.`;
}

const panel = await ui.registerPanel({
  id: "report",
  title: "Report",
  body: [
    { type: "select", id: "units", label: "Units", value: unitPref, options: [{ value: "document", label: "As the document" }, { value: "in", label: "Inches" }, { value: "mm", label: "Millimetres" }] },
    { type: "table", id: "counts", columns: ["Count", "Number"], rows: [], align: ["left", "right"] },
    { type: "note", id: "size", text: "" },
    { type: "table", id: "materials", columns: ["Material", "Amount"], rows: [], align: ["left", "right"] },
    { type: "note", id: "warnings", text: "" },
    { type: "button", id: "export", label: "Export report", primary: true },
  ],
  onChange: async (id, values) => {
    if (id !== "units") return;
    unitPref = /** @type {any} */ (values.units);
    await storage.app.set("units", unitPref);
    await refresh();
  },
  onAction: (id) => (id === "export" ? exportReport() : undefined),
});

let refreshing = Promise.resolve();
async function refresh() {
  refreshing = refreshing.then(async () => {
    const r = await gather();
    await panel.update({
      fields: {
        counts: { rows: countRows(r) },
        size: { text: sizeText(r) },
        materials: { rows: materialRows(r) },
        warnings: r.warnings.length ? { text: `Warnings: ${r.warnings.join(" ")}`, tone: "warning" } : { text: "No warnings.", tone: "info" },
      },
    });
  });
  return refreshing;
}

const esc = (/** @type {string | number} */ v) => {
  const s = String(v);
  return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
};

async function exportReport() {
  const r = await gather();
  const name = (r.doc.name ?? "Untitled").replace(/\.shz$/i, "");
  const md = [
    `# Model report: ${name}`,
    "",
    sizeText(r),
    "",
    "## Counts",
    "",
    "| Count | Number |",
    "|---|---:|",
    ...countRows(r).map(([k, v]) => `| ${k} | ${v} |`),
    "",
    "## Materials",
    "",
    "| Material | Amount |",
    "|---|---:|",
    ...materialRows(r).map(([k, v]) => `| ${k} | ${v} |`),
    "",
    "## Parts",
    "",
    "| Part | Qty | T | W | L | Material |",
    "|---|---:|---:|---:|---:|---|",
    ...r.cut.rows.map((p) => `| ${p.name} | ${p.qty} | ${len(p.thickness)} | ${len(p.width)} | ${len(p.length)} | ${p.material ?? ""} |`),
    "",
    "## Warnings",
    "",
    ...(r.warnings.length ? r.warnings.map((w) => `- ${w}`) : ["None."]),
    "",
  ].join("\n");
  const csv = [
    ["Section", "Item", "Value"],
    ...countRows(r).map(([k, v]) => ["Counts", k, v]),
    ["Size", "Overall", sizeText(r)],
    ...materialRows(r).map(([k, v]) => ["Materials", k, v]),
    ...r.cut.rows.map((p) => ["Parts", p.name, `${p.qty} x ${len(p.thickness)} x ${len(p.width)} x ${len(p.length)}`]),
    ...r.warnings.map((w) => ["Warnings", "", w]),
  ]
    .map((row) => row.map(esc).join(","))
    .join("\r\n");
  const a = await file.save({ name: `${name} report.md`, data: md, mime: "text/markdown", description: "Markdown" });
  const b = await file.save({ name: `${name} report.csv`, data: `﻿${csv}\r\n`, mime: "text/csv", description: "CSV" });
  const saved = [a, b].filter(Boolean).map((x) => /** @type {{ name: string }} */ (x).name);
  await ui.notify(saved.length ? `Saved ${saved.join(" and ")}.` : "Export cancelled.");
}

await ui.registerCommand({ id: "export", run: exportReport });
let timer = /** @type {ReturnType<typeof setTimeout> | null} */ (null);
const soon = () => {
  if (timer) clearTimeout(timer);
  timer = setTimeout(() => void refresh(), 150);
};
events.on("documentChanged", soon);
events.on("documentOpened", soon);
events.on("unitsChanged", soon);
await refresh();
