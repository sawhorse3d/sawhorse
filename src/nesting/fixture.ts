// A small nesting as the kernel returns it (lane 9), for the UI and format tests that run
// without the WASM kernel: two oak shelves on one 4/4 x 12 x 96 board, the back panel on a
// 1/4 in sheet, and a top that fits only across its grain. The WASM round-trip tests
// (`nesting.test.ts`) use the real kernel instead.
import type { Nesting, NestCut, Placement, StockDef, StockPiece } from "../kernel/types";

const IN = 0.0254;

export const LIBRARY_STOCK: StockDef[] = [
  { name: "4 x 8 sheet, 1/4", kind: "sheet", length: 96 * IN, width: 48 * IN, thickness: 0.25 * IN, grain: "length" },
  { name: "4/4 x 12 x 96", kind: "board", length: 96 * IN, width: 12 * IN, thickness: 0.8125 * IN, nominal: "4/4", grain: "length" },
];

function place(tag: string, name: string, x: number, y: number, w: number, l: number, node: number, row: number, copy: number): Placement {
  return { row, copy, tag, name, item: { node }, x: x * IN, y: y * IN, dx: l * IN, dy: w * IN, turned: false, grain: "length", size: [0.75 * IN, w * IN, l * IN] };
}

function cut(n: number, dir: "rip" | "crosscut", at: number, from: number, to: number, size: number): NestCut {
  return { n, dir, at: at * IN, from: from * IN, to: to * IN, size: size * IN, trim: false };
}

export function boardPiece(): StockPiece {
  const placements = [place("1a", "Shelf", 0, 0, 11, 34.5, 3, 0, 1), place("1b", "Shelf", 34.625, 0, 11, 34.5, 4, 0, 2)];
  const parts = 2 * 11 * 34.5 * IN * IN;
  const stock = 12 * 96 * IN * IN;
  return {
    index: 1,
    group: 0,
    seq: 1,
    count: 1,
    stock: "4/4 x 12 x 96",
    kind: "board",
    nominal: "4/4",
    material: "Oak",
    length: 96 * IN,
    width: 12 * IN,
    thickness: 0.8125 * IN,
    grain: "length",
    trim: 0,
    kerf: IN / 8,
    usable: [0, 0, 96 * IN, 12 * IN],
    placements,
    cuts: [cut(1, "crosscut", 34.5, 0, 12, 34.5), cut(2, "rip", 11, 0, 34.5, 11), cut(3, "crosscut", 69.125, 0, 12, 34.5), cut(4, "rip", 11, 34.625, 69.125, 11)],
    guillotine: true,
    parts_area: parts,
    stock_area: stock,
    waste_area: stock - parts,
    efficiency: parts / stock,
  };
}

export function sheetPiece(): StockPiece {
  const placements = [{ ...place("2", "Back panel", 0, 0, 34.5, 71.25, 7, 1, 1), size: [0.25 * IN, 34.5 * IN, 71.25 * IN] as [number, number, number] }];
  const parts = 34.5 * 71.25 * IN * IN;
  const stock = 48 * 96 * IN * IN;
  return {
    index: 2,
    group: 1,
    seq: 1,
    count: 1,
    stock: "4 x 8 sheet, 1/4",
    kind: "sheet",
    material: null,
    length: 96 * IN,
    width: 48 * IN,
    thickness: 0.25 * IN,
    grain: "length",
    trim: 0,
    kerf: IN / 8,
    usable: [0, 0, 96 * IN, 48 * IN],
    placements,
    cuts: [cut(1, "rip", 34.5, 0, 96, 34.5), cut(2, "crosscut", 71.25, 0, 34.5, 71.25)],
    guillotine: true,
    parts_area: parts,
    stock_area: stock,
    waste_area: stock - parts,
    efficiency: parts / stock,
  };
}

export function smallNesting(): Nesting {
  const pieces = [boardPiece(), sheetPiece()];
  const parts = pieces.reduce((s, p) => s + p.parts_area, 0);
  const stock = pieces.reduce((s, p) => s + p.stock_area, 0);
  return {
    algorithm: "guillotine",
    settings: { kerf: IN / 8, trim: 0, algorithm: "guillotine" },
    visible_only: false,
    library: { stocks: LIBRARY_STOCK, defaults: {}, settings: { kerf: IN / 8, trim: 0, algorithm: "guillotine" } },
    pieces,
    unplaced: [{ row: 2, copy: 1, tag: "3", name: "Top", item: { node: 9 }, size: [0.75 * IN, 11.25 * IN, 36 * IN], material: "Oak", stock: "4/4 x 12 x 96", sheet: false, reason: "grain" }],
    totals: {
      sheets: 1,
      boards: 1,
      parts_in: 4,
      placed: 3,
      unplaced: 1,
      parts_area: parts,
      stock_area: stock,
      efficiency: parts / stock,
      groups: [
        { stock: "4/4 x 12 x 96", kind: "board", nominal: "4/4", material: "Oak", length: 96 * IN, width: 12 * IN, thickness: 0.8125 * IN, count: 1, parts: 2, parts_area: pieces[0].parts_area, stock_area: pieces[0].stock_area, efficiency: pieces[0].efficiency, board_feet: 8 },
        { stock: "4 x 8 sheet, 1/4", kind: "sheet", material: null, length: 96 * IN, width: 48 * IN, thickness: 0.25 * IN, count: 1, parts: 1, parts_area: pieces[1].parts_area, stock_area: pieces[1].stock_area, efficiency: pieces[1].efficiency, board_feet: 0 },
      ],
      by_nominal: [{ nominal: "4/4", count: 1, board_feet: 8 }],
    },
    hash: "0123456789abcdef",
  };
}
