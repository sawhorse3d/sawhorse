import { describe, expect, it } from "vitest";
import { LONG_TOKEN, nameBreaks, nameRuns, sizeChars, sizePieces } from "./wrap";

// Lane 20b: where the Cut list pane's names, sizes and notes may wrap (no ellipsis anywhere).
describe("where a part's name may wrap", () => {
  it("before a # (the number stays with it)", () => {
    expect(nameBreaks("Agrupar#8")).toEqual(["Agrupar", "#8"]);
    expect(nameBreaks("Rodeo#2 drawer front left")).toEqual(["Rodeo", "#2 drawer front left"]);
    // Not at the start of a token, and once before a run of #.
    expect(nameBreaks("#8")).toEqual(["#8"]);
    expect(nameBreaks("A grup #2")).toEqual(["A grup #2"]);
    expect(nameBreaks("part##12")).toEqual(["part", "##12"]);
  });

  it("at camelCase joints", () => {
    expect(nameBreaks("drawerFrontLeft")).toEqual(["drawer", "Front", "Left"]);
    expect(nameBreaks("XMLParser")).toEqual(["XML", "Parser"]);
    expect(nameBreaks("shelfBack#3")).toEqual(["shelf", "Back", "#3"]);
    // An initial capital, all capitals, digits: no joint.
    expect(nameBreaks("Shelf")).toEqual(["Shelf"]);
    expect(nameBreaks("DOOR")).toEqual(["DOOR"]);
    expect(nameBreaks("2x4")).toEqual(["2x4"]);
  });

  it("after a run of _ or -", () => {
    expect(nameBreaks("Laura_Skin")).toEqual(["Laura_", "Skin"]);
    expect(nameBreaks("side-panel-left")).toEqual(["side-", "panel-", "left"]);
    expect(nameBreaks("a__b")).toEqual(["a__", "b"]);
    // Not before a space, and not at the end.
    expect(nameBreaks("edge- band_")).toEqual(["edge- band_"]);
  });

  it("never inside (3D)shelf2B, which has no joint, and joined the pieces are the name", () => {
    expect(nameBreaks("(3D)shelf2B")).toEqual(["(3D)shelf2B"]);
    for (const name of ["(3D)shelf2B", "Agrupar#8", "Gondulas Laterais", "Laura_Shirt2", "drawerFrontLeft", "ÉtagèreHaute#2", ""]) expect(nameBreaks(name).join("")).toBe(name);
    expect(nameBreaks("ÉtagèreHaute")).toEqual(["Étagère", "Haute"]);
  });

  it("a token of 30 or more characters is a run of its own (it may break anywhere, the last resort)", () => {
    expect(LONG_TOKEN).toBe(30);
    const long = "Abcdefghijklmnopqrstuvwxyzabcd"; // 30
    expect(nameRuns(`Top ${long} left`)).toEqual([
      { long: false, pieces: ["Top "] },
      { long: true, pieces: [long] },
      { long: false, pieces: [" left"] },
    ]);
    // 29 characters is not long; a long token keeps its joints (preferred to "anywhere").
    expect(nameRuns("Abcdefghijklmnopqrstuvwxyzabc")).toEqual([{ long: false, pieces: ["Abcdefghijklmnopqrstuvwxyzabc"] }]);
    expect(nameRuns("verylongcomponentname#123456789")).toEqual([{ long: true, pieces: ["verylongcomponentname", "#123456789"] }]);
    expect(nameRuns("Rodeo#2 drawer front left")).toEqual([{ long: false, pieces: ["Rodeo", "#2 drawer front left"] }]);
  });
});

describe("where a size may wrap", () => {
  it("between its whole number and its fraction, the fraction as one unit", () => {
    expect(sizePieces("22 13/16")).toEqual(["22", "13/16"]);
    expect(sizePieces("-22 13/16")).toEqual(["-22", "13/16"]);
    expect(sizePieces("13/16")).toEqual(["13/16"]);
    expect(sizePieces("2438.5")).toEqual(["2438.5"]);
    expect(sizePieces("0.0625")).toEqual(["0.0625"]);
  });

  it("a column is as wide as the whole size up to its cap, and never narrower than its widest unit", () => {
    // "22 13/16" fits a cap of 8: one line.
    expect(sizeChars("22 13/16", 8)).toBe(8);
    // Under a cap of 7 it wraps, "22" over "13/16": the column is 7 wide.
    expect(sizeChars("22 13/16", 7)).toBe(7);
    // "113 31/32" under 7: 7 (both units fit); a single unit wider than the cap widens it.
    expect(sizeChars("113 31/32", 7)).toBe(7);
    expect(sizeChars("123456.5", 7)).toBe(8);
    expect(sizeChars("3/4", 7)).toBe(3);
    expect(sizeChars("1 15/32", 4)).toBe(5);
  });
});
