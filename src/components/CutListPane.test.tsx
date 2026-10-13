import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { bookshelf } from "../cutlist/fixture";
import { grainArrows, grainOverlay } from "../cutlist/arrows";
import { columnWidths, CutListPane, pickRows, rowKey, type CutListPaneProps } from "./CutListPane";
import { CutListReport } from "./CutListReport";
import { REPORT_COLUMNS, reportCells } from "../cutlist/format";
import { readPdf } from "../cutlist/pdf";
import { reportPdf } from "../cutlist/documents";
import type { CutList, CutRow } from "../kernel/types";

afterEach(cleanup);

/** The row names of a handler's last call. */
const names = (fn: unknown) => ((fn as ReturnType<typeof vi.fn>).mock.lastCall![0] as CutRow[]).map((r) => r.name);

function props(over: Partial<CutListPaneProps> = {}): CutListPaneProps {
  return {
    list: bookshelf(),
    rounding: "sixteenth",
    onRounding: vi.fn(),
    visibleOnly: false,
    onVisibleOnly: vi.fn(),
    selected: new Set(),
    onSelectRows: vi.fn(),
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
    expect(screen.getByTestId("cutlist-note").textContent).toMatch(/Edge banding is not included; the sheet count here is a rough area estimate \(Nesting lays/);
  });

  it("clicks select a row's pieces; right-click sets the grain, the stock, sheet or lumber", () => {
    const p = props({ selected: new Set([rowKey(bookshelf().rows[1])]) });
    render(<CutListPane {...p} />);
    const shelf = screen.getByText("Shelf").closest("tr")!;
    expect(shelf.getAttribute("aria-selected")).toBe("true");
    fireEvent.click(screen.getByText("Side").closest("tr")!);
    expect(names(p.onSelectRows)).toEqual(["Side"]);
    // Ctrl toggles the shelf off (it was the selection).
    fireEvent.click(shelf, { ctrlKey: true });
    expect(names(p.onSelectRows)).toEqual([]);
    fireEvent.contextMenu(screen.getByText("Top").closest("tr")!);
    const menu = screen.getByRole("menu", { name: "Cut list: Top" });
    expect(within(menu).getByRole("menuitemradio", { name: /across/ }).getAttribute("aria-checked")).toBe("true");
    fireEvent.click(within(menu).getByRole("menuitemradio", { name: /along the length/ }));
    expect(p.onGrain).toHaveBeenLastCalledWith([expect.objectContaining({ name: "Top" })], "length");
    // Stock: a dialog with the name and how it is counted.
    fireEvent.contextMenu(screen.getByText("Back panel").closest("tr")!);
    fireEvent.click(screen.getByRole("menuitem", { name: /Stock/ }));
    const dialog = screen.getByRole("dialog", { name: "Stock for Back panel" });
    const input = within(dialog).getByRole("textbox") as HTMLInputElement;
    expect(input.value).toBe("1/4 birch ply");
    fireEvent.change(input, { target: { value: "1/4 Baltic birch ply" } });
    fireEvent.change(within(dialog).getByLabelText("Counted as"), { target: { value: "sheet" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Set stock" }));
    expect(p.onStock).toHaveBeenLastCalledWith([expect.objectContaining({ name: "Back panel" })], "1/4 Baltic birch ply", true);
    // Mark the sides as sheet goods (keeps their material's name).
    fireEvent.contextMenu(screen.getByText("Side").closest("tr")!);
    fireEvent.click(screen.getByRole("menuitem", { name: "Mark as sheet goods" }));
    expect(p.onStock).toHaveBeenLastCalledWith([expect.objectContaining({ name: "Side" })], "keep", true);
  });

  it("Shift picks a range of rows, Ctrl adds one; right-click on a picked row acts on them all", () => {
    const list = bookshelf();
    const p = props();
    const { rerender } = render(<CutListPane {...p} />);
    const tr = (name: string) => screen.getByText(name, { selector: ".name" }).closest("tr")!;
    fireEvent.click(tr("Side"));
    fireEvent.click(tr("Top"), { shiftKey: true });
    expect(names(p.onSelectRows)).toEqual(["Side", "Shelf", "Top"]);
    // The app selects their pieces; the rows come back picked.
    const picked = new Set([0, 1, 2].map((k) => rowKey(list.rows[k])));
    rerender(<CutListPane {...p} selected={picked} />);
    fireEvent.click(tr("Back panel"), { ctrlKey: true });
    expect(names(p.onSelectRows)).toEqual(["Side", "Shelf", "Top", "Back panel"]);
    // Ctrl+Shift adds a range (from the last plain or Ctrl click).
    fireEvent.click(tr("Toe kick"), { ctrlKey: true, shiftKey: true });
    expect(names(p.onSelectRows)).toEqual(["Side", "Shelf", "Top", "Toe kick", "Back panel"]);
    // Right-click a picked row: the menu is for all three, titled with them, all marked.
    fireEvent.contextMenu(tr("Shelf"));
    const menu = screen.getByRole("menu", { name: "Cut list: 3 parts" });
    expect(menu.querySelector(".ctxtitle")?.textContent).toBe("3 parts: Side, Shelf, Top");
    for (const n of ["Side", "Shelf", "Top"]) expect(tr(n).className).toContain("menu");
    expect(tr("Toe kick").className).not.toContain("menu");
    // Mixed grain: no choice is ticked.
    expect(within(menu).getAllByRole("menuitemradio").map((b) => b.getAttribute("aria-checked"))).toEqual(["false", "false", "false"]);
    fireEvent.click(within(menu).getByRole("menuitemradio", { name: /No grain/ }));
    expect((p.onGrain as ReturnType<typeof vi.fn>).mock.lastCall![0].map((r: CutRow) => r.name)).toEqual(["Side", "Shelf", "Top"]);
    // Stock for all three in one dialog (they share Oak).
    fireEvent.contextMenu(tr("Top"));
    fireEvent.click(screen.getByRole("menuitem", { name: /Stock/ }));
    const dialog = screen.getByRole("dialog", { name: "Stock for 3 parts" });
    const input = within(dialog).getByRole("textbox") as HTMLInputElement;
    expect(input.value).toBe("Oak");
    fireEvent.change(input, { target: { value: "White oak" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Set stock" }));
    const [rows, stock, sheet] = (p.onStock as ReturnType<typeof vi.fn>).mock.lastCall!;
    expect([rows.map((r: CutRow) => r.name), stock, sheet]).toEqual([["Side", "Shelf", "Top"], "White oak", null]);
    // Right-click a row that is not picked: just that row.
    fireEvent.contextMenu(tr("Toe kick"));
    expect(screen.getByRole("menu", { name: "Cut list: Toe kick" })).toBeTruthy();
  });

  it("pickRows: plain, Ctrl, Shift, Ctrl+Shift", () => {
    const rows = bookshelf().rows;
    const k = (n: number) => rowKey(rows[n]);
    const nm = (rs: CutRow[]) => rs.map((r) => r.name);
    expect(nm(pickRows(rows, new Set([k(0)]), rows[3], k(0), { shift: false, toggle: false }))).toEqual(["Toe kick"]);
    expect(nm(pickRows(rows, new Set([k(0)]), rows[3], k(0), { shift: true, toggle: false }))).toEqual(["Side", "Shelf", "Top", "Toe kick"]);
    expect(nm(pickRows(rows, new Set([k(4)]), rows[1], k(4), { shift: true, toggle: false }))).toEqual(["Shelf", "Top", "Toe kick", "Back panel"]);
    expect(nm(pickRows(rows, new Set([k(0)]), rows[0], k(0), { shift: false, toggle: true }))).toEqual([]);
    expect(nm(pickRows(rows, new Set([k(0)]), rows[2], null, { shift: true, toggle: false }))).toEqual(["Top"]);
    expect(nm(pickRows(rows, new Set([k(4)]), rows[1], k(0), { shift: true, toggle: true }))).toEqual(["Side", "Shelf", "Back panel"]);
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

describe("Cut list pane fit (300 px)", () => {
  it("fixes the column widths from the longest value, caps the sizes, and leaves the rest to Part and Material", () => {
    const list = bookshelf();
    // 3/4, 11 1/4, 71 1/4: 3, 6 and 6 characters; the grain mark beside W (the top) and L.
    expect(columnWidths(list, "fraction")).toEqual(["calc(3ch + 10px)", "calc(3ch + 10px)", "calc(6ch + 21px)", "calc(6ch + 21px)"]);
    // Thirty-seconds over 100 in are capped; the full size is in the cell's tooltip.
    const long = { ...list, rounding: "thirty_second" as const, rows: [{ ...list.rows[0], grain: "none" as const, qty: 12345, steps: [47, 3647, 3231] as [number, number, number] }] };
    expect(columnWidths(long, "fraction")).toEqual(["calc(3ch + 10px)", "calc(7ch + 10px)", "calc(8ch + 10px)", "calc(9ch + 10px)"]);
    render(<CutListPane {...props({ list: long, rounding: "thirty_second" })} />);
    const cells = [...screen.getByText("Side").closest("tr")!.querySelectorAll("td")];
    expect(cells[3].textContent).toBe("113 31/32");
    expect(cells[3].getAttribute("title")).toBe("113 31/32 in");
    expect(cells[4].getAttribute("title")).toBeNull();
    expect(screen.getByRole("table", { name: "Cut list" }).className).toContain("fit");
  });

  it("Part and Material carry their full text as a tooltip; tabular figures", () => {
    const list = bookshelf();
    list.rows[0] = { ...list.rows[0], name: "A side panel with a name far too long for the pane", hidden: 1, material: "Quartersawn white oak, select and better" };
    render(<CutListPane {...props({ list })} />);
    const row = screen.getByText(/A side panel/).closest("tr")!;
    expect(row.querySelector("td.part")!.getAttribute("title")).toBe("A side panel with a name far too long for the pane (1 hidden)");
    expect(row.querySelector("td.mat")!.getAttribute("title")).toBe("Quartersawn white oak, select and better");
    expect(document.querySelectorAll("col")).toHaveLength(6);
  });

  it("the right-click menu is titled with the row's name and the row stays marked while it is open", () => {
    render(<CutListPane {...props()} />);
    const top = screen.getByText("Top").closest("tr")!;
    expect(top.className).not.toContain("menu");
    fireEvent.contextMenu(top);
    const menu = screen.getByRole("menu", { name: "Cut list: Top" });
    expect(menu.querySelector(".ctxtitle")?.textContent).toBe("Top");
    expect(screen.getByText("Top", { selector: ".name" }).closest("tr")!.className).toContain("menu");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(screen.getByText("Top", { selector: ".name" }).closest("tr")!.className).not.toContain("menu");
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
    expect(report.querySelector("footer")?.textContent).toMatch(/Nesting lays the parts out on real sheets/);
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


describe("Parts that are not solids (lane 15)", () => {
  /** The bookshelf with an imported bench seat that is not a solid, a dropped part, and loose faces. */
  function mixed(): CutList {
    const list = bookshelf();
    list.rows[0] = { ...list.rows[0], name: "Bench seat", not_solid: true };
    list.rows[3] = { ...list.rows[3], included: false };
    list.totals = { ...list.totals, not_solid: 1, excluded: 1, loose_faces: 312 };
    return list;
  }

  it("flags the part in the pane, says what is outside parts, and offers to make parts of it", () => {
    const p = props({ list: mixed(), onInclude: vi.fn(), onGroupLoose: vi.fn() });
    render(<CutListPane {...p} />);
    const seat = screen.getByText("Bench seat").closest("tr")!;
    expect(seat.getAttribute("data-not-solid")).toBe("true");
    expect(seat.querySelector("small")!.textContent).toContain("not solid (sizes from bounds)");
    const note = screen.getByTestId("cutlist-geometry");
    expect(note.textContent).toContain("1 part not solid (sizes from bounds) · 312 faces outside parts · 1 part left out of the totals");
    fireEvent.click(within(note).getByRole("button", { name: "Make parts from loose faces" }));
    expect(p.onGroupLoose).toHaveBeenCalledTimes(1);
  });

  it("an include box per part drops it or counts it again, without selecting the row", () => {
    const p = props({ list: mixed(), onInclude: vi.fn() });
    render(<CutListPane {...p} />);
    const kick = screen.getByLabelText("Include Toe kick") as HTMLInputElement;
    expect(kick.checked).toBe(false);
    expect(kick.closest("tr")!.className).toContain("out");
    fireEvent.click(kick);
    expect(p.onInclude).toHaveBeenLastCalledWith([expect.objectContaining({ name: "Toe kick" })], true);
    fireEvent.click(screen.getByLabelText("Include Shelf"));
    expect(p.onInclude).toHaveBeenLastCalledWith([expect.objectContaining({ name: "Shelf" })], false);
    expect(p.onSelectRows).not.toHaveBeenCalled();
    // The right-click menu says the same.
    fireEvent.contextMenu(screen.getByText("Shelf").closest("tr")!);
    fireEvent.click(screen.getByRole("menuitem", { name: "Leave out of the totals" }));
    expect(p.onInclude).toHaveBeenLastCalledWith([expect.objectContaining({ name: "Shelf" })], false);
  });

  it("every output carries the flag: CSV, report, labels", async () => {
    const { cutListCsv, parseCsv, csvCellText } = await import("../cutlist/csv");
    const { labelsPdf } = await import("../cutlist/documents");
    const list = mixed();
    const csv = parseCsv(cutListCsv(list)).map((r) => r.map(csvCellText));
    const seat = csv.find((r) => r[0] === "Bench seat")!;
    expect(seat[13]).toContain("not solid (sizes from bounds)");
    expect(csv.find((r) => r[0] === "Toe kick")![13]).toContain("left out of the totals");
    const report = readPdf(reportPdf(list, "Bench")).text.flat().join(" ");
    expect(report).toContain("not solid (sizes from bounds)");
    expect(report).toContain("312 faces outside parts");
    const labels = readPdf(labelsPdf(list, "Bench")).text.flat().join(" ");
    expect(labels).toContain("not solid (sizes from bounds)");
    expect(labels).not.toContain("Toe kick");
  });
});
