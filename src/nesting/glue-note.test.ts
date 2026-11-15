// #56 point 4: a glued part's cut list row says what it is glued from, and a board's count
// of parts counts the part once however many strips of it the board holds.
import { describe, expect, it } from "vitest";
import { readPdf } from "../cutlist/pdf";
import { row } from "../cutlist/fixture";
import type { GlueUp, Nesting } from "../kernel/types";
import { nestingPdf } from "./booklet";
import { gluedNesting, smallNesting } from "./fixture";
import { glueNote, pieceCount } from "./format";
import { pieceSvg } from "./svg";

const IN = 0.0254;

/** The shelf row as the cut list gives it: two copies, nodes 3 and 4. */
function shelf(qty = 2) {
  return row("Shelf", qty, [12, 240, 552], {
    occurrences: [3, 4, 5].slice(0, qty).map((node) => ({ item: { node }, visible: true, handed: false }) as never),
  });
}

function glued(over: Partial<GlueUp>): GlueUp {
  return { row: 0, copy: 1, tag: "1a", name: "Shelf", item: { node: 3 }, stock: "4/4 x 12 x 96", widths: [12 * IN, 3.125 * IN], ...over };
}

function withGlueUps(gs: GlueUp[]): Nesting {
  const n = smallNesting();
  n.glue_ups = gs;
  return n;
}

describe("#56: the count of parts on a board", () => {
  it("reads as before when the board holds no strips", () => {
    const n = smallNesting();
    expect(pieceCount(n.pieces[0])).toBe("2 parts");
    expect(pieceCount(n.pieces[1])).toBe("1 part");
  });

  it("counts a glued part once, and says how many strips or pieces it is cut as", () => {
    const n = gluedNesting();
    // Two strips of 1a and the whole 1b: two parts, three pieces of wood.
    expect(pieceCount(n.pieces[0])).toBe("2 parts in 3 pieces");
    // Only the strips of one part.
    const strips = { ...n.pieces[0], placements: n.pieces[0].placements.slice(0, 2) };
    expect(pieceCount(strips)).toBe("1 part in 2 strips");
    // One strip of a part whose other strips are on another board.
    expect(pieceCount({ ...strips, placements: strips.placements.slice(1) })).toBe("1 part in 1 strip");
    // Strips of two parts.
    const two = { ...strips, placements: [...strips.placements, ...strips.placements.map((q) => ({ ...q, tag: q.tag.replace("1a", "1c") }))] };
    expect(pieceCount(two)).toBe("2 parts in 4 strips");
  });

  it("is what the booklet's Stock pieces list and the SVG's title say", () => {
    const n = gluedNesting();
    const first = readPdf(nestingPdf(n, "Bookshelf", "sixteenth")).text[0].join("\n");
    expect(first).toContain("2 parts in 3 pieces");
    const svg = pieceSvg(n, n.pieces[0], { title: "Bookshelf", rounding: "sixteenth" });
    expect(svg).toContain("2 parts in 3 pieces");
    // A board with no strips reads as before in both.
    expect(first).toContain("1 part");
    expect(pieceSvg(n, n.pieces[1], { title: "Bookshelf", rounding: "sixteenth" })).toContain(" 1 part ");
  });
});

describe("#56: the note on a glued part's cut list row", () => {
  it("says how many strips and from which board, when every copy is glued the same", () => {
    const n = withGlueUps([glued({}), glued({ copy: 2, tag: "1b", item: { node: 4 } })]);
    expect(glueNote(shelf(), 0, n)).toBe("glued from 2 (4/4 x 12 x 96)");
  });

  it("is absent with no nesting, no glue-ups, or for another row", () => {
    expect(glueNote(shelf(), 0, null)).toBeNull();
    expect(glueNote(shelf(), 0, undefined)).toBeNull();
    expect(glueNote(shelf(), 0, smallNesting())).toBeNull();
    expect(glueNote(shelf(), 1, withGlueUps([glued({})]))).toBeNull();
  });

  it("never labels a row whose parts are not the glued ones (a nesting of an older model)", () => {
    // Row 0 now holds other parts: the glue-up's item is not among them.
    expect(glueNote(shelf(), 0, withGlueUps([glued({ item: { node: 99 } })]))).toBeNull();
    expect(glueNote(shelf(), 0, withGlueUps([glued({ item: { object: 3 } })]))).toBeNull();
  });

  it("says how many copies when only some are glued, and splits copies glued differently", () => {
    expect(glueNote(shelf(), 0, withGlueUps([glued({})]))).toBe("1 glued from 2 (4/4 x 12 x 96)");
    const n = withGlueUps([
      glued({}),
      glued({ copy: 2, item: { node: 4 } }),
      glued({ copy: 3, item: { node: 5 }, stock: "4/4 x 6 x 96", widths: [6 * IN, 6 * IN, 3.25 * IN] }),
    ]);
    expect(glueNote(shelf(3), 0, n)).toBe("2 glued from 2 (4/4 x 12 x 96), 1 from 3 (4/4 x 6 x 96)");
  });
});
