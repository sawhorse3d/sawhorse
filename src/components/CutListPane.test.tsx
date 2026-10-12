import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { bookshelf } from "../cutlist/fixture";
import { grainArrows, grainOverlay } from "../cutlist/arrows";
import { CutListPane, rowKey, type CutListPaneProps } from "./CutListPane";
import { CutListReport } from "./CutListReport";
import { REPORT_COLUMNS, reportCells } from "../cutlist/format";
import { readPdf } from "../cutlist/pdf";
import { reportPdf } from "../cutlist/documents";
import type { CutList } from "../kernel/types";

afterEach(cleanup);

function props(over: Partial<CutListPaneProps> = {}): CutListPaneProps {
  return {
    list: bookshelf(),
    rounding: "sixteenth",
    onRounding: vi.fn(),
    visibleOnly: false,
    onVisibleOnly: vi.fn(),
    selected: new Set(),
    onSelectRow: vi.fn(),
    onGrain: vi.fn(),
    onStock: vi.fn(),
    onCsv: vi.fn(),
    onPdf: vi.fn(),
    onPrint: vi.fn(),
    onLabels: vi.fn(),
    ...over,
  };
}

describe("Cut list pane", () => {
  it("offers Follow units first (saying what it comes to), and a Pin for the choice", () => {
    const p = props({ rounding: "follow", follows: "half_mm", onPin: vi.fn(), pinned: false });
    render(<CutListPane {...p} />);
    const sel = screen.getByLabelText("Round sizes to") as HTMLSelectElement;
    expect([...sel.options].map((o) => o.textContent)).toEqual(["Follow units (0.5 mm)", "1/16 in", "1/32 in", "0.5 mm"]);
    expect(sel.value).toBe("follow");
    // Nothing to pin while following the units.
    expect((screen.getByRole("button", { name: "Pin" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(sel, { target: { value: "thirty_second" } });
    expect(p.onRounding).toHaveBeenLastCalledWith("thirty_second");
    cleanup();
    const q = props({ rounding: "thirty_second", onPin: vi.fn(), pinned: false });
    render(<CutListPane {...q} />);
    const pin = screen.getByRole("button", { name: "Pin" });
    expect(pin.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(pin);
    expect(q.onPin).toHaveBeenLastCalledWith(true);
  });

  it("writes decimal inches when the list is in the decimal style", () => {
    render(<CutListPane {...props({ list: { ...bookshelf(), style: "decimal" } })} />);
    const row = screen.getByText("Side").closest("tr")!;
    expect([...row.querySelectorAll("td")].slice(2, 5).map((td) => td.textContent)).toEqual(["0.75", "11.25", "71.25"]);
  });

  it("is the mock's table and tiles: Part, Qty, T, W, L, Material; board feet and sheets", () => {
    render(<CutListPane {...props()} />);
    const table = screen.getByRole("table", { name: "Cut list" });
    expect(within(table).getAllByRole("columnheader").map((h) => h.textContent)).toEqual(["Part", "Qty", "T", "W", "L", "Material"]);
    const rows = within(table).getAllByRole("row").slice(1);
    const text = rows.map((r) => [...r.querySelectorAll("td")].map((td) => td.querySelector(".name")?.textContent ?? td.textContent));
    expect(text).toEqual([
      ["Side", "2", "3/4", "11 1/4", "71 1/4", "Oak"],
      ["Shelf", "4", "3/4", "11", "34 1/2", "Oak"],
      ["Top", "1", "3/4", "11 1/4", "36", "Oak"],
      ["Toe kick", "1", "3/4", "3 1/2", "34 1/2", "Oak"],
      ["Back panel", "1", "1/4", "34 1/2", "71 1/4", "1/4 birch ply"],
    ]);
    // The grain mark sits beside the size it runs along: L for most, W for the top.
    expect(rows[0].querySelectorAll("td")[4].querySelector("i.grain")).toBeTruthy();
    expect(rows[2].querySelectorAll("td")[3].querySelector("i.grain")).toBeTruthy();
    expect(rows[2].querySelectorAll("td")[4].querySelector("i.grain")).toBeNull();
    expect(rows[4].textContent).toContain("sheet");
    expect(screen.getByTestId("tile-board-feet").textContent).toBe("18.99board feet, Oak");
    expect(screen.getByTestId("tile-sheets").textContent).toBe("1sheet 4×8 1/4 birch ply, 1/4");
    expect((screen.getByRole("button", { name: /Nesting/ }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByTestId("cutlist-note").textContent).toMatch(/Kerf, edge banding and nesting \(cutting diagrams\) are not included yet/);
  });

  it("clicks select a row's pieces; right-click sets the grain, the stock, sheet or lumber", () => {
    const p = props({ selected: new Set([rowKey(bookshelf().rows[1])]) });
    render(<CutListPane {...p} />);
    const shelf = screen.getByText("Shelf").closest("tr")!;
    expect(shelf.getAttribute("aria-selected")).toBe("true");
    fireEvent.click(screen.getByText("Side").closest("tr")!);
    expect(p.onSelectRow).toHaveBeenLastCalledWith(expect.objectContaining({ name: "Side" }), "replace");
    fireEvent.click(shelf, { ctrlKey: true });
    expect(p.onSelectRow).toHaveBeenLastCalledWith(expect.objectContaining({ name: "Shelf" }), "toggle");
    fireEvent.contextMenu(screen.getByText("Top").closest("tr")!);
    const menu = screen.getByRole("menu", { name: "Cut list: Top" });
    expect(within(menu).getByRole("menuitemradio", { name: /across/ }).getAttribute("aria-checked")).toBe("true");
    fireEvent.click(within(menu).getByRole("menuitemradio", { name: /along the length/ }));
    expect(p.onGrain).toHaveBeenLastCalledWith(expect.objectContaining({ name: "Top" }), "length");
    // Stock: a dialog with the name and how it is counted.
    fireEvent.contextMenu(screen.getByText("Back panel").closest("tr")!);
    fireEvent.click(screen.getByRole("menuitem", { name: /Stock/ }));
    const dialog = screen.getByRole("dialog", { name: "Stock for Back panel" });
    const input = within(dialog).getByRole("textbox") as HTMLInputElement;
    expect(input.value).toBe("1/4 birch ply");
    fireEvent.change(input, { target: { value: "1/4 Baltic birch ply" } });
    fireEvent.change(within(dialog).getByLabelText("Counted as"), { target: { value: "sheet" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Set stock" }));
    expect(p.onStock).toHaveBeenLastCalledWith(expect.objectContaining({ name: "Back panel" }), "1/4 Baltic birch ply", true);
    // Mark the sides as sheet goods (keeps their material's name).
    fireEvent.contextMenu(screen.getByText("Side").closest("tr")!);
    fireEvent.click(screen.getByRole("menuitem", { name: "Mark as sheet goods" }));
    expect(p.onStock).toHaveBeenLastCalledWith(expect.objectContaining({ name: "Side" }), null, true);
  });

  it("rounding, visible only and the exports reach their handlers", () => {
    const p = props();
    render(<CutListPane {...p} />);
    fireEvent.change(screen.getByLabelText("Round sizes to"), { target: { value: "thirty_second" } });
    expect(p.onRounding).toHaveBeenCalledWith("thirty_second");
    fireEvent.click(screen.getByLabelText("Visible only"));
    expect(p.onVisibleOnly).toHaveBeenCalledWith(true);
    for (const [name, fn] of [["CSV", p.onCsv], ["PDF", p.onPdf], ["Print", p.onPrint], ["Labels", p.onLabels]] as const) {
      fireEvent.click(screen.getByRole("button", { name }));
      expect(fn).toHaveBeenCalledTimes(1);
    }
  });

  it("an empty model says how parts come about; everything hidden says to untick Visible only", () => {
    const empty: CutList = { ...bookshelf(), rows: [], totals: { ...bookshelf().totals, parts: 0, pieces: 0, board_feet: 0, lumber: [], sheets: [], sheet_count: 0 } };
    const { rerender } = render(<CutListPane {...props({ list: empty })} />);
    expect(screen.getByTestId("cutlist-empty").textContent).toMatch(/Make Component/);
    expect((screen.getByRole("button", { name: "CSV" }) as HTMLButtonElement).disabled).toBe(true);
    rerender(<CutListPane {...props({ list: { ...empty, visible_only: true, totals: { ...empty.totals, hidden: 3 } }, visibleOnly: true })} />);
    expect(screen.getByTestId("cutlist-empty").textContent).toMatch(/Untick Visible only/);
  });
});

describe("Printable report", () => {
  it("renders every row, grain and the totals into a print-only article on the page body", () => {
    render(<CutListReport list={bookshelf()} title="Bookshelf" />);
    const report = screen.getByTestId("print-report");
    expect(report.parentElement).toBe(document.body);
    expect(report.querySelector("h1")?.textContent).toBe("Cut list: Bookshelf");
    const rows = [...report.querySelectorAll("tbody tr")].map((r) => [...r.querySelectorAll("td")].map((td) => td.textContent));
    expect([...report.querySelectorAll("thead th")].map((th) => th.textContent)).toEqual(["Part", "Qty", "T", "W", "L", "Material", "Grain", "Bd ft / area", "Notes"]);
    expect(rows[2]).toEqual(["Top", "1", "3/4", "11 1/4", "36", "Oak", "across W", "2.11 bd ft", "grain across W"]);
    expect(rows[0].slice(6)).toEqual(["along L", "8.35 bd ft", ""]);
    expect(rows[4][5]).toBe("1/4 birch ply (sheet)");
    expect(rows[4][7]).toBe("17.07 sq ft");
    expect(report.querySelector(".totals")?.textContent).toContain("18.99 board feet, Oak");
    expect(report.querySelector("footer")?.textContent).toMatch(/nesting/);
  });
});

describe("Grain arrows", () => {
  it("one arrow on each broad face of every drawn piece, along L (or W), none for no grain", () => {
    const list = bookshelf();
    const at = (c: number[]) => ({ item: { node: 1 }, visible: true, handed: false, axes: [[1, 0, 0], [0, 1, 0], [0, 0, 1]] as CutList["rows"][0]["occurrences"][0]["axes"], size: [0.019, 0.286, 1.81] as [number, number, number], center: c as [number, number, number], shaped: false });
    list.rows[0].occurrences = [at([0, 0, 0]), { ...at([1, 0, 0]), visible: false }];
    list.rows[2].occurrences = [at([0, 0, 2])];
    list.rows[3].grain = "none";
    list.rows[3].occurrences = [at([0, 0, 3])];
    const a = grainArrows(list);
    // Side: 1 visible piece x 2 faces x 4 segments (shaft, two head strokes, tail) x 2 points.
    expect(a.normal).toHaveLength(16);
    // The shaft runs along L (blue here), on the face across T (red).
    expect(a.normal[0][0]).toBeCloseTo(0.0095 + 0.0008, 9);
    expect(a.normal[1][2] - a.normal[0][2]).toBeGreaterThan(0.5);
    // The top's grain runs along W (green).
    expect(a.across).toHaveLength(16);
    expect(Math.abs(a.across[1][1] - a.across[0][1])).toBeGreaterThan(0.1);
    expect(a.across[1][2]).toBeCloseTo(2, 9);
    expect(grainOverlay(list).map((o) => [o.color, o.depthTest])).toEqual([
      ["--grain", true],
      ["--grain-across", true],
    ]);
    expect(grainOverlay(null)).toEqual([]);
  });
});

describe("Report consistency", () => {
  it("the PDF and the print view write the same columns and the same cells", () => {
    const list = bookshelf();
    list.rows[1] = { ...list.rows[1], hidden: 1, shaped: true, scaled: true };
    list.rows[3] = { ...list.rows[3], grain: "none", grain_set: true };
    render(<CutListReport list={list} title="Bookshelf" />);
    const report = screen.getByTestId("print-report");
    const head = [...report.querySelectorAll("thead th")].map((th) => th.textContent);
    expect(head).toEqual([...REPORT_COLUMNS]);
    const printed = [...report.querySelectorAll("tbody tr")].map((r) => [...r.querySelectorAll("td")].map((td) => td.textContent));
    expect(printed).toEqual(list.rows.map((r) => reportCells(r, list)));
    expect(printed[1][8]).toBe("shaped; 1 hidden; scaled");
    expect(printed[3][8]).toBe("no grain");
    expect(printed[3][6]).toBe("none");
    const pdf = readPdf(reportPdf(list, "Bookshelf")).text.flat();
    for (const c of REPORT_COLUMNS) expect(pdf, c).toContain(c.toUpperCase());
    for (const row of printed)
      for (const cell of row.filter(Boolean)) {
        // The PDF wraps long notes over lines; every word is there.
        const found = pdf.includes(cell) || cell.split(" ").every((w) => pdf.some((t) => t.split(" ").includes(w)));
        expect(found, cell).toBe(true);
      }
  });
});

