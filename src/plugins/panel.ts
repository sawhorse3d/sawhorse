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

export const PANEL_LIMITS = { fields: 100, text: 2000, rows: 500, columns: 12, options: 200, image: 1024 * 1024 };

const IMAGE = /^data:image\/(png|jpeg|gif|webp|svg\+xml);base64,[A-Za-z0-9+/=\s]+$/;
const FIELD_ID = /^[A-Za-z][A-Za-z0-9_-]{0,39}$/;

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

function str(v: unknown, where: string, max = PANEL_LIMITS.text, required = true): string {
  if (v === undefined && !required) return "";
  if (typeof v !== "string") throw new TypeError(`${where} must be text`);
  return v.slice(0, max);
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
      const src = str(raw.src, `${where}.src`, PANEL_LIMITS.image);
      if (!IMAGE.test(src)) throw new TypeError(`${where}.src must be a base64 data: URL of a png, jpeg, gif, webp or svg image`);
      return drop({ type: "image", id: fieldId(raw.id, where, false), src, alt: str(raw.alt, `${where}.alt`, 200), width: num(raw.width, `${where}.width`) });
    }
    default:
      throw new TypeError(`${where}.type must be one of text, number, select, checkbox, button, table, note, image (got ${JSON.stringify(raw.type)})`);
  }
}

/** A panel body, checked: at most 100 fields, ids unique. */
export function checkBody(raw: unknown): PanelField[] {
  if (!Array.isArray(raw)) throw new TypeError("a panel body is a list of fields");
  if (raw.length > PANEL_LIMITS.fields) throw new TypeError(`a panel holds at most ${PANEL_LIMITS.fields} fields`);
  const fields = raw.map((f, i) => checkField(f, `body[${i}]`));
  const ids = fields.flatMap((f) => (f.id ? [f.id] : []));
  const dup = ids.find((id, i) => ids.indexOf(id) !== i);
  if (dup) throw new TypeError(`two fields have the id "${dup}"`);
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
    next = next.map((f, i) => (f.id && isObj(patches[f.id]) ? checkField({ ...f, ...(patches[f.id] as Record<string, unknown>), type: f.type, id: f.id }, `body[${i}]`) : f));
    for (const id of Object.keys(patches)) if (!next.some((f) => f.id === id)) throw new TypeError(`there is no field "${id}"`);
  }
  if (patch.values !== undefined) {
    if (!isObj(patch.values)) throw new TypeError("values must be an object of values by field id");
    const values = patch.values;
    next = next.map((f) => (f.id && f.id in values && (f.type === "text" || f.type === "number" || f.type === "select" || f.type === "checkbox") ? checkField({ ...f, value: values[f.id] }, f.id) : f));
  }
  return next;
}
