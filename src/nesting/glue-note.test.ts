// #56 point 4: a glued part's cut list row says what it is glued from, and a board's count
// of parts counts the part once however many strips of it the board holds.
import { describe, expect, it } from "vitest";
import { readPdf, textWidth } from "../cutlist/pdf";
import { row } from "../cutlist/fixture";
import type { GlueUp, Nesting } from "../kernel/types";
import { nestingPdf } from "./booklet";
import { gluedNesting, smallNesting } from "./fixture";
import { glueNote, pieceCount, pieceYield } from "./format";
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
    // Strips of two parts.
    const two = { ...strips, placements: [...strips.placements, ...strips.placements.map((q) => ({ ...q, tag: q.tag.replace("1a", "1c") }))] };
    expect(pieceCount(two)).toBe("2 parts in 4 strips");
  });

  it("names the part a board holds only some strips of, and never counts it as a part there", () => {
    // A part's strips can land on different boards (each too long to share one): the kernel
    // counts the part once, on the board with its first strip, so no board may call it a part.
    const n = gluedNesting();
    const [s1, s2, whole] = n.pieces[0].placements;
    const on = (...placements: typeof s1[]) => pieceCount({ ...n.pieces[0], placements });
    expect(on(s1)).toBe("1 strip of 1a");
    expect(on(s2, whole)).toBe("1 part, 1 strip of 1a");
    // Two of a part's three strips, beside a part glued whole on this board.
    const three = [s1, s2].map((q) => ({ ...q, tag: q.tag.replace("1a", "1c"), strip: { n: q.strip!.n, of: 3 } }));
    expect(on(s1, s2, ...three)).toBe("1 part in 2 strips, 2 strips of 1c");
  });

  it("keeps each board's yield on the page of its title and count, at any page break", () => {
    // Sixty glued boards, each count running into its yield, with yields that differ, so a
    // yield pushed alone onto the next page would be found there.
    // Where a row falls against the foot of the page depends on what is above the list, so
    // lengthen the glue-up list above it a line at a time.
    for (let glued = 1; glued <= 14; glued++) {
      const count = 60;
      const n = gluedNesting();
      n.glue_ups = Array.from({ length: glued }, (_, k) => ({ ...n.glue_ups![0], tag: `1${"abcdefghijklmn"[k]}` }));
      const base = n.pieces[0];
      n.pieces = Array.from({ length: count }, (_, k) => ({ ...base, index: k + 1, seq: k + 1, count, efficiency: (k + 10) / 100 }));
      const pages = readPdf(nestingPdf(n, "Bookshelf", "sixteenth")).text.map((t) => t.join("\n"));
      for (const p of n.pieces) {
        const page = pages.find((t) => t.includes(`${p.index}. Board ${p.seq} of ${count}`));
        expect(page, `${count} boards: board ${p.index}`).toBeTruthy();
        expect(page, `${count} boards: board ${p.index}'s yield`).toContain(`${(p.efficiency * 100).toFixed(1)}% used`);
      }
    }
  });

  /** Each text the booklet's first page draws, with where it starts. */
  function placed(pdf: Uint8Array): { t: string; x: number; y: number }[] {
    const raw = Buffer.from(pdf).toString("latin1");
    const page = /stream\n([\s\S]*?)\nendstream/.exec(raw)![1];
    return [...page.matchAll(/Tf ([\d.-]+) ([\d.-]+) Td \(((?:[^()\\]|\\.)*)\) Tj/g)].map((m) => ({ t: m[3].replace(/\\267/g, "·"), x: Number(m[1]), y: Number(m[2]) }));
  }

  it("leaves a booklet row with no strips on one line, as before, however close its count comes to the yield", () => {
    // Thirteen parts on a sheet: "13 parts" ends about a point before "57.4% used · 13.65 sq
    // ft waste" starts. A booklet with no glue-up reads as it did before #56.
    const n = smallNesting();
    const sheet = n.pieces[1];
    sheet.placements = Array.from({ length: 13 }, () => sheet.placements[0]);
    sheet.efficiency = 0.574;
    sheet.waste_area = 13.65 * 0.3048 * 0.3048;
    const items = placed(nestingPdf(n, "Bookshelf", "sixteenth"));
    const count = items.find((i) => i.t === "13 parts")!;
    const yieldAt = items.find((i) => i.t === "57.4% used · 13.65 sq ft waste")!;
    expect(count.y).toBe(yieldAt.y);
    expect(count.x + textWidth("13 parts", 10)).toBeLessThan(yieldAt.x);
  });

  it("wraps a long count after its commas in the booklet, whole, with the yield under it", () => {
    // A board with the two strips of 1a and one strip each of three parts too long to share
    // a board: wider than the count's column, so it takes lines that break only after a comma.
    const n = gluedNesting();
    const [s1, s2] = n.pieces[0].placements;
    const one = (tag: string) => ({ ...s1, tag: `${tag}.1`, strip: { n: 1, of: 2 } });
    n.pieces[0].placements = [s1, s2, one("1c"), one("1d"), one("1e")];
    const text = pieceCount(n.pieces[0]);
    expect(text).toBe("1 part in 2 strips, 1 strip of 1c, 1 strip of 1d, 1 strip of 1e");
    const items = placed(nestingPdf(n, "Bookshelf", "sixteenth"));
    const title = items.find((i) => i.t.startsWith("1. Board 1 of 1"))!;
    const yieldAt = items.find((i) => i.t === pieceYield(n.pieces[0], "sixteenth"))!;
    const lines = items.filter((i) => i.x === title.x + 340 && i.y <= title.y && i.y > yieldAt.y);
    expect(lines.length).toBeGreaterThan(1);
    expect(lines.map((i) => i.t).join(" ")).toBe(text);
    expect(lines.every((i, k) => i.y === title.y - 14 * k && !i.t.endsWith("...") && !/ of$/.test(i.t))).toBe(true);
    expect(yieldAt.y).toBe(lines[lines.length - 1].y - 14);
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
