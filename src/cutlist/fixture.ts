// A cut list as the kernel returns it for the user guide's bookshelf (crates/kernel/tests/
// cutlist.rs builds the same model), for the UI and export tests.
import type { CutList, CutRow } from "../kernel/types";

const IN = 0.0254;
export function row(name: string, qty: number, steps: [number, number, number], extra: Partial<CutRow> = {}): CutRow {
  const rounded = steps.map((s) => (s * IN) / 16) as [number, number, number];
  const sheet = extra.sheet ?? false;
  return {
    key: { definition: qty },
    target: { definition: qty },
    name,
    qty,
    total: qty,
    hidden: 0,
    handed: 0,
    size: rounded,
    rounded,
    steps,
    shaped: false,
    scaled: false,
    grain: "length",
    grain_set: false,
    material: "Oak",
    stock_set: false,
    sheet,
    sheet_set: false,
    board_feet: sheet ? 0 : ((steps[0] * steps[1] * steps[2]) / (16 ** 3 * 144)) * qty,
    area_m2: sheet ? rounded[1] * rounded[2] * qty : 0,
    occurrences: [],
    ...extra,
  };
}

/** The component bookshelf as the kernel lists it (crates/kernel/tests/cutlist.rs). */
export function bookshelf(): CutList {
  const rows = [
    row("Side", 2, [12, 180, 1140]),
    row("Shelf", 4, [12, 176, 552]),
    row("Top", 1, [12, 180, 576], { grain: "width", grain_set: true }),
    row("Toe kick", 1, [12, 56, 552]),
    row("Back panel", 1, [4, 552, 1140], { sheet: true, material: "1/4 birch ply", stock_set: true }),
  ];
  const bf = rows.reduce((s, r) => s + r.board_feet, 0);
  return {
    rounding: "sixteenth",
    visible_only: false,
    rows,
    totals: {
      parts: 5,
      pieces: 9,
      hidden: 0,
      board_feet: bf,
      lumber: [{ material: "Oak", board_feet: bf }],
      sheets: [{ material: "1/4 birch ply", thickness: 0.25 * IN, steps: 4, area_m2: rows[4].area_m2, sheets: 1, oversize: false }],
      sheet_count: 1,
    },
    sheet_size: [48 * IN, 96 * IN],
    hash: "0",
  };
}

