// Plugin panels are declarative: a plugin sends a list of fields (text, number with units,
// select, checkbox, button, table, note, image) and the app draws them with its own
// components (src/components/PluginPanel.tsx). Nothing a plugin sends becomes markup: every
// field is checked here and only known properties of known types are kept.

export type FieldValue = string | number | boolean | null;

export type PanelField =
  | { type: "text"; id: string; label: string; value: string; placeholder?: string; disabled?: boolean }
  | { type: "number"; id: string; label: string; value: number | null; unit: "length" | "none" | "count" | "angle"; min?: number; max?: number; step?: number; disabled?: boolean }
  | { type: "select"; id: string; label: string; value: string; options: { value: string; label: string }[]; disabled?: boolean }
  | { type: "checkbox"; id: string; label: string; value: boolean; disabled?: boolean }
  | { type: "button"; id: string; label: string; primary?: boolean; disabled?: boolean }
  | { type: "table"; id?: string; columns: string[]; rows: string[][]; align?: ("left" | "right")[] }
  | { type: "note"; id?: string; text: string; tone: "info" | "warning" | "error" }
  | { type: "image"; id?: string; src: string; alt: string; width?: number };

export const PANEL_LIMITS = { fields: 100, text: 2000, rows: 500, columns: 12, options: 200, image: 256 * 1024 };

/**
 * Lane 16: what one panel may hold in all, whatever its fields: table cells, characters of
 * text (labels, values, notes, cells, options), images and their encoded size, and the
 * pixels an image may decode to (sniffed from its header before the browser decodes it).
 */
export const PANEL_BUDGET = { cells: 5000, chars: 256 * 1024, images: 4, imageBytes: 512 * 1024, imageSide: 2048 };

// SVG is refused (lane 16): its size is not in a header and it can describe any bitmap size.
const IMAGE = /^data:image\/(png|jpeg|gif|webp);base64,[A-Za-z0-9+/=\s]+$/;
const FIELD_ID = /^[A-Za-z][A-Za-z0-9_-]{0,39}$/;

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

/** A copy that does not keep a longer string alive (a slice can point into its parent). */
const fresh = (s: string): string => (s.length ? JSON.parse(JSON.stringify(s)) : s);

function str(v: unknown, where: string, max = PANEL_LIMITS.text, required = true): string {
  if (v === undefined && !required) return "";
  if (typeof v !== "string") throw new TypeError(`${where} must be text`);
  return v.length > max ? fresh(v.slice(0, max)) : v;
}

/** An image's pixel size from its header (PNG, JPEG, GIF, WebP), or null when it cannot be read. */
export function imageSize(b: Uint8Array): { width: number; height: number } | null {
  const u16be = (o: number) => (b[o] << 8) | b[o + 1];
  const u32be = (o: number) => ((b[o] << 24) >>> 0) + (b[o + 1] << 16) + (b[o + 2] << 8) + b[o + 3];
  if (b.length >= 24 && b[0] === 0x89 && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47) return { width: u32be(16), height: u32be(20) };
  if (b.length >= 10 && b[0] === 0x47 && b[1] === 0x49 && b[2] === 0x46) return { width: b[6] | (b[7] << 8), height: b[8] | (b[9] << 8) };
  if (b.length >= 30 && b[0] === 0x52 && b[1] === 0x49 && b[2] === 0x46 && b[3] === 0x46 && b[8] === 0x57 && b[9] === 0x45 && b[10] === 0x42 && b[11] === 0x50) {
    const kind = String.fromCharCode(b[12], b[13], b[14], b[15]);
    if (kind === "VP8 ") return { width: (b[26] | (b[27] << 8)) & 0x3fff, height: (b[28] | (b[29] << 8)) & 0x3fff };
    if (kind === "VP8L") {
      const bits = b[21] | (b[22] << 8) | (b[23] << 16) | (b[24] << 24);
      return { width: (bits & 0x3fff) + 1, height: ((bits >>> 14) & 0x3fff) + 1 };
    }
    if (kind === "VP8X") return { width: 1 + (b[24] | (b[25] << 8) | (b[26] << 16)), height: 1 + (b[27] | (b[28] << 8) | (b[29] << 16)) };
    return null;
  }
  if (b.length >= 4 && b[0] === 0xff && b[1] === 0xd8) {
    let o = 2;
    while (o + 9 < b.length) {
      if (b[o] !== 0xff) return null;
      const marker = b[o + 1];
      const len = u16be(o + 2);
      // SOF0..SOF15, except DHT (C4), JPG (C8) and DAC (CC).
      if (marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc) return { width: u16be(o + 7), height: u16be(o + 5) };
      if (len < 2) return null;
      o += 2 + len;
    }
  }
  return null;
}

/** Base64 to bytes (atob is in every browser and in Node). */
function fromBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

function num(v: unknown, where: string): number | undefined {
  if (v === undefined || v === null) return undefined;
  if (typeof v !== "number" || !Number.isFinite(v)) throw new TypeError(`${where} must be a finite number`);
  return v;
}

function bool(v: unknown): boolean | undefined {
  return v === undefined ? undefined : !!v;
}

function fieldId(v: unknown, where: string, required: boolean): string | undefined {
  if (v === undefined && !required) return undefined;
  if (typeof v !== "string" || !FIELD_ID.test(v)) throw new TypeError(`${where}.id must be letters, digits, "-" or "_", starting with a letter`);
  return v;
}

const drop = <T extends Record<string, unknown>>(o: T): T => {
  for (const k of Object.keys(o)) if (o[k] === undefined) delete o[k];
  return o;
};

/** One field, checked and reduced to what the renderer knows. */
export function checkField(raw: unknown, where: string): PanelField {
  if (!isObj(raw)) throw new TypeError(`${where} must be an object with a "type"`);
  const disabled = bool(raw.disabled);
  switch (raw.type) {
    case "text":
      return drop({ type: "text", id: fieldId(raw.id, where, true)!, label: str(raw.label, `${where}.label`, 64), value: str(raw.value, `${where}.value`, PANEL_LIMITS.text, false), placeholder: raw.placeholder === undefined ? undefined : str(raw.placeholder, `${where}.placeholder`, 64), disabled });
    case "number": {
      const unit = raw.unit ?? "none";
      if (!["length", "none", "count", "angle"].includes(unit as string)) throw new TypeError(`${where}.unit must be "length", "none", "count" or "angle"`);
      return drop({ type: "number", id: fieldId(raw.id, where, true)!, label: str(raw.label, `${where}.label`, 64), value: num(raw.value, `${where}.value`) ?? null, unit: unit as "length", min: num(raw.min, `${where}.min`), max: num(raw.max, `${where}.max`), step: num(raw.step, `${where}.step`), disabled });
    }
    case "select": {
      if (!Array.isArray(raw.options) || raw.options.length === 0) throw new TypeError(`${where}.options must be a non-empty list`);
      const options = raw.options.slice(0, PANEL_LIMITS.options).map((o, i) => {
        if (!isObj(o)) throw new TypeError(`${where}.options[${i}] must be { value, label }`);
        return { value: str(o.value, `${where}.options[${i}].value`, 200), label: str(o.label, `${where}.options[${i}].label`, 100) };
      });
      const value = raw.value === undefined ? options[0].value : str(raw.value, `${where}.value`, 200);
      return drop({ type: "select", id: fieldId(raw.id, where, true)!, label: str(raw.label, `${where}.label`, 64), value, options, disabled });
    }
    case "checkbox":
      return drop({ type: "checkbox", id: fieldId(raw.id, where, true)!, label: str(raw.label, `${where}.label`, 100), value: !!raw.value, disabled });
    case "button":
      return drop({ type: "button", id: fieldId(raw.id, where, true)!, label: str(raw.label, `${where}.label`, 64), primary: bool(raw.primary), disabled });
    case "table": {
      if (!Array.isArray(raw.columns) || raw.columns.length > PANEL_LIMITS.columns) throw new TypeError(`${where}.columns must be a list of at most ${PANEL_LIMITS.columns} headings`);
      const columns = raw.columns.map((c, i) => str(c, `${where}.columns[${i}]`, 64));
      if (!Array.isArray(raw.rows)) throw new TypeError(`${where}.rows must be a list of rows`);
      const rows = raw.rows.slice(0, PANEL_LIMITS.rows).map((r, i) => {
        if (!Array.isArray(r)) throw new TypeError(`${where}.rows[${i}] must be a list of cells`);
        return columns.map((_c, k) => {
          const cell = r[k];
          return cell === undefined || cell === null ? "" : typeof cell === "number" ? String(cell) : str(cell, `${where}.rows[${i}][${k}]`, 200);
        });
      });
      const align = Array.isArray(raw.align) ? raw.align.slice(0, columns.length).map((a) => (a === "right" ? "right" : "left") as "left" | "right") : undefined;
      return drop({ type: "table", id: fieldId(raw.id, where, false), columns, rows, align });
    }
    case "note": {
      const tone = raw.tone ?? "info";
      if (!["info", "warning", "error"].includes(tone as string)) throw new TypeError(`${where}.tone must be "info", "warning" or "error"`);
      return drop({ type: "note", id: fieldId(raw.id, where, false), text: str(raw.text, `${where}.text`), tone: tone as "info" });
    }
    case "image": {
      if (typeof raw.src !== "string") throw new TypeError(`${where}.src must be text`);
      // Refused whole when too large (a cut data: URL is a broken image anyway).
      if (raw.src.length > PANEL_LIMITS.image) throw new TypeError(`${where}.src is larger than ${PANEL_LIMITS.image / 1024} KB`);
      const src = raw.src;
      if (!IMAGE.test(src)) throw new TypeError(`${where}.src must be a base64 data: URL of a png, jpeg, gif or webp image`);
      const size = imageSize(fromBase64(src.slice(src.indexOf(",") + 1)));
      if (!size) throw new TypeError(`${where}.src is not a readable png, jpeg, gif or webp image`);
      if (size.width > PANEL_BUDGET.imageSide || size.height > PANEL_BUDGET.imageSide || size.width < 1 || size.height < 1)
        throw new TypeError(`${where}.src is ${size.width} x ${size.height} pixels; at most ${PANEL_BUDGET.imageSide} on a side`);
      return drop({ type: "image", id: fieldId(raw.id, where, false), src, alt: str(raw.alt, `${where}.alt`, 200), width: num(raw.width, `${where}.width`) });
    }
    default:
      throw new TypeError(`${where}.type must be one of text, number, select, checkbox, button, table, note, image (got ${JSON.stringify(raw.type)})`);
  }
}

/** What a checked field costs against PANEL_BUDGET. */
function costOf(f: PanelField): { cells: number; chars: number; images: number; imageBytes: number } {
  let chars = 0;
  for (const v of Object.values(f)) if (typeof v === "string") chars += v.length;
  if (f.type === "select") for (const o of f.options) chars += o.value.length + o.label.length;
  if (f.type === "table") {
    for (const c of f.columns) chars += c.length;
    for (const r of f.rows) for (const c of r) chars += c.length;
    return { cells: f.rows.length * Math.max(1, f.columns.length), chars, images: 0, imageBytes: 0 };
  }
  if (f.type === "image") return { cells: 0, chars: chars - f.src.length, images: 1, imageBytes: f.src.length };
  return { cells: 0, chars, images: 0, imageBytes: 0 };
}

/** Throw when the fields together are over the panel's budget (PANEL_BUDGET). */
export function checkBudget(fields: PanelField[]): void {
  const spent = { cells: 0, chars: 0, images: 0, imageBytes: 0 };
  for (const f of fields) {
    const c = costOf(f);
    spent.cells += c.cells;
    spent.chars += c.chars;
    spent.images += c.images;
    spent.imageBytes += c.imageBytes;
  }
  if (spent.cells > PANEL_BUDGET.cells) throw new TypeError(`a panel holds at most ${PANEL_BUDGET.cells} table cells in all`);
  if (spent.chars > PANEL_BUDGET.chars) throw new TypeError(`a panel holds at most ${PANEL_BUDGET.chars / 1024} K characters of text in all`);
  if (spent.images > PANEL_BUDGET.images) throw new TypeError(`a panel holds at most ${PANEL_BUDGET.images} images`);
  if (spent.imageBytes > PANEL_BUDGET.imageBytes) throw new TypeError(`a panel's images take at most ${PANEL_BUDGET.imageBytes / 1024} KB in all`);
}

/** A panel body, checked: at most 100 fields, ids unique, and the panel's budget in all (lane 16). */
export function checkBody(raw: unknown): PanelField[] {
  if (!Array.isArray(raw)) throw new TypeError("a panel body is a list of fields");
  if (raw.length > PANEL_LIMITS.fields) throw new TypeError(`a panel holds at most ${PANEL_LIMITS.fields} fields`);
  const fields = raw.map((f, i) => checkField(f, `body[${i}]`));
  checkBudget(fields);
  const ids = new Set<string>();
  for (const f of fields) {
    if (!f.id) continue;
    if (ids.has(f.id)) throw new TypeError(`two fields have the id "${f.id}"`);
    ids.add(f.id);
  }
  return fields;
}

/** The values a panel reports: every input field by id. */
export function valuesOf(fields: PanelField[]): Record<string, FieldValue> {
  const out: Record<string, FieldValue> = {};
  for (const f of fields) if (f.type === "text" || f.type === "number" || f.type === "select" || f.type === "checkbox") out[f.id] = f.value;
  return out;
}

/** Apply `ui.updatePanel`: field patches by id (re-checked whole) and values. */
export function patchFields(fields: PanelField[], patch: { fields?: unknown; values?: unknown }): PanelField[] {
  let next = fields;
  if (patch.fields !== undefined) {
    if (!isObj(patch.fields)) throw new TypeError("fields must be an object of patches by field id");
    const patches = patch.fields;
    next = next.map((f, i) => (f.id && Object.prototype.hasOwnProperty.call(patches, f.id) && isObj(patches[f.id]) ? checkField({ ...f, ...(patches[f.id] as Record<string, unknown>), type: f.type, id: f.id }, `body[${i}]`) : f));
    const known = new Set(next.flatMap((f) => (f.id ? [f.id] : [])));
    for (const id of Object.keys(patches)) if (!known.has(id)) throw new TypeError(`there is no field "${id.slice(0, 40)}"`);
  }
  if (patch.values !== undefined) {
    if (!isObj(patch.values)) throw new TypeError("values must be an object of values by field id");
    const values = patch.values;
    // Own keys only (lane 16): a field named "constructor" is not in `values` because
    // Object.prototype has a constructor.
    next = next.map((f) => (f.id && Object.prototype.hasOwnProperty.call(values, f.id) && (f.type === "text" || f.type === "number" || f.type === "select" || f.type === "checkbox") ? checkField({ ...f, value: values[f.id] }, f.id) : f));
  }
  if (next !== fields) checkBudget(next);
  return next;
}
