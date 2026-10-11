// The tool rail. Keys are the SketchUp-2017 defaults people's hands already know
// (ADR-0003); glyphs are our own (see components/glyphs.tsx).

export type ToolId =
  | "select" | "erase"
  | "line" | "rect" | "circle" | "arc"
  | "pushpull" | "move" | "rotate" | "scale" | "offset" | "follow"
  | "tape" | "dim" | "paint";

export interface ToolDef {
  id: ToolId;
  /** Short label under the glyph. */
  label: string;
  /** KeyboardEvent.key that activates the tool (lower case; " " is Space). */
  key: string;
  /** What the key cap in the rail shows. */
  keyCap: string;
  /** Status-bar hint shown when the tool is active. */
  hint: string;
  /** Value Control Box label, or null when the tool takes no typed value. */
  vcb: string | null;
}

export const TOOL_GROUPS: ToolDef[][] = [
  [
    { id: "select", label: "Select", key: " ", keyCap: "␣", hint: "Click an entity. Drag for a window. Double-click a group to edit it.", vcb: null },
    { id: "erase", label: "Erase", key: "e", keyCap: "E", hint: "Click or drag over edges to erase. Shift hides, Ctrl softens.", vcb: null },
  ],
  [
    { id: "line", label: "Line", key: "l", keyCap: "L", hint: "Click to start. Type a length and press Enter. Closed loops form faces.", vcb: "Length" },
    { id: "rect", label: "Rect", key: "r", keyCap: "R", hint: "Click two corners. Type 24,12 for exact size.", vcb: "Dimensions" },
    { id: "circle", label: "Circle", key: "c", keyCap: "C", hint: "Click center, then radius. Type a radius and press Enter.", vcb: "Radius" },
    { id: "arc", label: "Arc", key: "a", keyCap: "A", hint: "Two-point arc. Click ends, then bulge. Type a radius or 12s for segments.", vcb: "Bulge" },
  ],
  [
    { id: "pushpull", label: "Push", key: "p", keyCap: "P", hint: "Click a face and move. Type 3/4 for 3/4 in. Double-click repeats the last distance.", vcb: "Distance" },
    { id: "move", label: "Move", key: "m", keyCap: "M", hint: "Click, move, click. Ctrl copies. Type 36 then x3 for an array.", vcb: "Distance" },
    { id: "rotate", label: "Rotate", key: "q", keyCap: "Q", hint: "Set the protractor on a face, click the start angle, then the end. Type 90.", vcb: "Angle" },
    { id: "scale", label: "Scale", key: "s", keyCap: "S", hint: "Drag a grip. Type 2 for 2x, or 48in for an absolute size.", vcb: "Scale" },
    { id: "offset", label: "Offset", key: "f", keyCap: "F", hint: "Click a face, move inward or outward. Type 3/4.", vcb: "Distance" },
    { id: "follow", label: "Follow", key: "w", keyCap: "W", hint: "Select a path, then click a profile face to extrude it along the path.", vcb: null },
  ],
  [
    { id: "tape", label: "Tape", key: "t", keyCap: "T", hint: "Click two points to measure. Click an edge and move for a guide.", vcb: "Length" },
    { id: "dim", label: "Dim", key: "d", keyCap: "D", hint: "Click two points, then place the dimension line.", vcb: null },
    { id: "paint", label: "Paint", key: "b", keyCap: "B", hint: "Click a face to paint the active material.", vcb: null },
  ],
];

export const TOOLS: ToolDef[] = TOOL_GROUPS.flat();

const BY_KEY = new Map(TOOLS.map((t) => [t.key, t]));
const BY_ID = new Map(TOOLS.map((t) => [t.id, t]));

export function toolForKey(key: string): ToolDef | undefined {
  return BY_KEY.get(key.length === 1 ? key.toLowerCase() : key);
}

export function toolById(id: ToolId): ToolDef {
  const t = BY_ID.get(id);
  if (!t) throw new Error(`unknown tool ${id}`);
  return t;
}

/** Whether a keydown should start typing into the Value Control Box (SketchUp-style: just type). */
export function startsValueEntry(key: string): boolean {
  return key.length === 1 && /[0-9.\-/'",~]/.test(key);
}
