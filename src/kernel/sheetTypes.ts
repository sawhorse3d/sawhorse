// Drawing sheets as the kernel stores them (lane 12, crates/kernel/src/sheets.rs and
// paper.rs). Paper coordinates are metres from the paper's top-left corner, y down; a rect
// is [x, y, width, height].
import type { V3 } from "./types";

export type PaperSizeKind = "letter" | "legal" | "tabloid" | "a4" | "a3" | "a2" | "custom";
export type PaperSize = { kind: Exclude<PaperSizeKind, "custom"> } | { kind: "custom"; width: number; height: number };
export type PaperUnits = "inches" | "millimetres";

export type Scale = { kind: "ratio"; paper: number; model: number } | { kind: "architect"; inches_per_foot: number } | { kind: "nts" };

export type LengthFormat = { kind: "fraction"; den: number } | { kind: "decimal"; places: number } | { kind: "feet_inches"; den: number } | { kind: "millimetres"; places: number };

export interface TitleBlock {
  show?: boolean;
  title?: string;
  project?: string;
  /** Empty: "n of N". */
  sheet?: string;
  date?: string;
  /** Empty: the viewports' scale. */
  scale?: string;
  drawn_by?: string;
  revision?: string;
}

export interface SheetSetup {
  name: string;
  paper: PaperSize;
  landscape?: boolean;
  /** Top, right, bottom, left (metres). */
  margins?: [number, number, number, number];
  units?: PaperUnits;
  /** Snap grid on paper, metres (0: none). */
  grid?: number;
  format?: LengthFormat;
  title?: TitleBlock;
}

export type StdView = "top" | "bottom" | "front" | "back" | "left" | "right" | "iso";

export interface CameraSpec {
  eye: V3;
  target: V3;
  up: V3;
  parallel?: boolean;
  fov?: number;
}

export type ViewportSource = { from: "scene"; name: string } | { from: "standard"; view: StdView } | { from: "camera"; camera: CameraSpec };

export type DimKind = "horizontal" | "vertical" | "aligned" | "angular" | "radius" | "diameter";
export type DimStyle = "arrows" | "ticks" | "dots";
export type CutColumn = "part" | "qty" | "thickness" | "width" | "length" | "size" | "material" | "grain" | "amount" | "notes";

/** Where a model point sits on a solid, so it follows the solid's edits (the kernel's
 * `ModelAnchor`): the corner where faces `at` meet, or the point `t` of the way from it to
 * the corner where faces `to` meet. */
export interface ModelAnchor {
  object: number;
  at: number[];
  to?: number[];
  t?: number;
}

export type LeaderEnd = { on: "paper"; at: [number, number] } | { on: "model"; viewport: number; point: V3; anchor?: ModelAnchor | null };

export type ItemKind =
  | {
      kind: "viewport";
      rect: [number, number, number, number];
      source: ViewportSource;
      scale: Scale;
      centre?: [number, number] | null;
      hidden_lines?: boolean;
      caption?: string | null;
      border?: boolean;
      /** The scale it had when it last showed a view to scale, kept while it shows an
       * isometric or perspective one (NTS), so going back restores it (lane 12c). */
      ortho_scale?: Scale | null;
    }
  | { kind: "dimension"; viewport: number; dim: DimKind; points: V3[]; anchors?: (ModelAnchor | null)[]; offset: number; style?: DimStyle; text?: string | null }
  | { kind: "label"; at: [number, number]; text: string; size: number; leader?: LeaderEnd | null }
  | { kind: "table"; rect: [number, number, number, number]; columns: CutColumn[]; decimal?: boolean; visible_only?: boolean; title?: string | null }
  | { kind: "image"; rect: [number, number, number, number]; path: string }
  | { kind: "line"; a: [number, number]; b: [number, number]; width: number; dashed?: boolean }
  | { kind: "rect"; rect: [number, number, number, number]; width: number; fill?: [number, number, number] | null };

export type SheetItem = { id: number } & ItemKind;

export interface Sheet extends SheetSetup {
  items: SheetItem[];
  next_item: number;
}

/** A sheet with its public id, in the document's order. */
export interface SheetRow {
  id: number;
  sheet: Sheet;
}

export type SheetOp =
  | { op: "add_sheet"; setup: SheetSetup }
  | { op: "set_sheet"; sheet: number; setup: SheetSetup }
  | { op: "delete_sheet"; sheet: number }
  | { op: "move_sheet"; sheet: number; to: number }
  | { op: "add_sheet_item"; sheet: number; item: ItemKind }
  | { op: "set_sheet_item"; sheet: number; item: number; kind: ItemKind }
  | { op: "move_sheet_items"; sheet: number; items: number[]; by: [number, number] }
  | { op: "delete_sheet_items"; sheet: number; items: number[] }
  | { op: "order_sheet_item"; sheet: number; item: number; to: number };
