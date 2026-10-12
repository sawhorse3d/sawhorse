import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { smallNesting } from "../nesting/fixture";
import { NestingSection, type NestingSectionProps } from "./NestingSection";
import { StockDialog, type StockDialogProps } from "./StockDialog";
import { CutListPane, type CutListPaneProps } from "./CutListPane";
import { bookshelf } from "../cutlist/fixture";

afterEach(cleanup);

const IN = 0.0254;

function props(over: Partial<NestingSectionProps> = {}): NestingSectionProps {
  return {
    nesting: smallNesting(),
    rounding: "sixteenth",
    units: "frac-in",
    onSettings: vi.fn(),
    onStock: vi.fn(),
    onHover: vi.fn(),
    onSelect: vi.fn(),
    onSvg: vi.fn(),
    onDxf: vi.fn(),
    onPdf: vi.fn(),
    ...over,
  };
}

describe("Nesting section", () => {
  it("shows the tiles, the parts no stock takes, and a diagram per piece", () => {
    render(<NestingSection {...props()} />);
    expect(screen.getByTestId("tile-nest-sheets").textContent).toContain("1");
    expect(screen.getByTestId("tile-nest-sheets").textContent).toContain("4 x 8 sheet, 1/4");
    expect(screen.getByTestId("tile-nest-boards").textContent).toContain("1 × 4/4, 8.0 bd ft");
    expect(screen.getByTestId("tile-nest-yield").textContent).toContain("3 of 4 parts placed");
    const alert = screen.getByTestId("nest-unplaced");
    expect(alert.textContent).toContain("Top");
    expect(alert.textContent).toContain("only with its grain across");
    const figs = document.querySelectorAll("figure.nest-piece");
    expect(figs.length).toBe(2);
    expect(figs[0].querySelectorAll("g.part").length).toBe(2);
    expect(figs[0].querySelectorAll("line.cut").length).toBe(4);
    expect(figs[0].textContent).toContain("Board 1 of 1");
    expect(within(figs[0] as HTMLElement).getAllByText(/Rip to 11 in/).length).toBe(2);
    expect(screen.getByTestId("nest-note").textContent).toContain("Kerf 1/8 in");
  });

  it("hovering a part lights it in the model, clicking selects it, and the model lights it back", () => {
    const p = props();
    const { rerender } = render(<NestingSection {...p} />);
    const part = document.querySelector('g.part[data-tag="1b"]')!;
    fireEvent.mouseEnter(part);
    expect(p.onHover).toHaveBeenLastCalledWith(expect.objectContaining({ tag: "1b", item: { node: 4 } }));
    expect(part.getAttribute("class")).toContain("on");
    fireEvent.click(part);
    expect(p.onSelect).toHaveBeenCalledWith(expect.objectContaining({ tag: "1b" }));
    fireEvent.mouseLeave(part.closest("svg")!);
    expect(p.onHover).toHaveBeenLastCalledWith(null);
    rerender(<NestingSection {...p} lit="2" />);
    expect(document.querySelector('g.part[data-tag="2"]')!.getAttribute("class")).toContain("on");
  });

  it("kerf and trim take typed lengths, the saw switches, and bad input says why", () => {
    vi.useFakeTimers();
    const p = props();
    render(<NestingSection {...p} />);
    const kerf = screen.getByLabelText("Kerf") as HTMLInputElement;
    expect(kerf.value).toBe("1/8 in");
    fireEvent.change(kerf, { target: { value: "3/32" } });
    fireEvent.keyDown(kerf, { key: "Enter" });
    expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: (3 / 32) * IN, trim: 0, algorithm: "guillotine" });
    // A pause after typing commits too.
    const trim = screen.getByLabelText("Trim") as HTMLInputElement;
    fireEvent.change(trim, { target: { value: "1/4" } });
    vi.advanceTimersByTime(800);
    expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: IN / 8, trim: IN / 4, algorithm: "guillotine" });
    const calls = (p.onSettings as ReturnType<typeof vi.fn>).mock.calls.length;
    fireEvent.change(kerf, { target: { value: "2 in" } });
    fireEvent.keyDown(kerf, { key: "Enter" });
    expect((p.onSettings as ReturnType<typeof vi.fn>).mock.calls.length).toBe(calls);
    expect(kerf.closest("label")!.getAttribute("title")).toContain("0 to 1/2 in");
    fireEvent.change(screen.getByLabelText("Nesting algorithm"), { target: { value: "maxrects" } });
    expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: IN / 8, trim: 0, algorithm: "maxrects" });
    vi.useRealTimers();
  });

  it("exports: every SVG, one piece's SVG, both DXFs and the PDF", () => {
    const p = props();
    render(<NestingSection {...p} />);
    const row = document.querySelector(".nesting > .btnrow")!;
    fireEvent.click(within(row as HTMLElement).getByRole("button", { name: "SVG" }));
    expect(p.onSvg).toHaveBeenLastCalledWith(null);
    fireEvent.click(within(row as HTMLElement).getByRole("button", { name: "DXF" }));
    expect(p.onDxf).toHaveBeenLastCalledWith("lwpolyline");
    fireEvent.click(within(row as HTMLElement).getByRole("button", { name: "DXF R12" }));
    expect(p.onDxf).toHaveBeenLastCalledWith("r12");
    fireEvent.click(within(row as HTMLElement).getByRole("button", { name: "PDF" }));
    expect(p.onPdf).toHaveBeenCalled();
    const fig = document.querySelectorAll("figure.nest-piece")[1] as HTMLElement;
    fireEvent.click(within(fig).getByRole("button", { name: "SVG" }));
    expect(p.onSvg).toHaveBeenLastCalledWith(expect.objectContaining({ index: 2 }));
    fireEvent.click(screen.getByRole("button", { name: "Stock…" }));
    expect(p.onStock).toHaveBeenCalled();
  });

  it("says why when there is no nesting, and nothing to export", () => {
    render(<NestingSection {...props({ nesting: null, problem: "Nesting is worked out by the kernel, which is not loaded." })} />);
    expect(screen.getByText(/not loaded/)).toBeTruthy();
    expect((screen.getByRole("button", { name: "PDF" }) as HTMLButtonElement).disabled).toBe(true);
  });
});

describe("Stock dialog", () => {
  function dprops(over: Partial<StockDialogProps> = {}): StockDialogProps {
    return {
      library: smallNesting().library,
      materials: ["Oak", "3/4 birch ply"],
      rounding: "sixteenth",
      units: "frac-in",
      onDefine: vi.fn(),
      onDelete: vi.fn(),
      onDefault: vi.fn(),
      onReset: vi.fn(),
      onClose: vi.fn(),
      ...over,
    };
  }

  it("sets a material's stock, adds, edits and deletes stock", () => {
    const p = dprops();
    render(<StockDialog {...p} />);
    fireEvent.change(screen.getByLabelText("Stock for Oak"), { target: { value: "4/4 x 12 x 96" } });
    expect(p.onDefault).toHaveBeenLastCalledWith("Oak", "4/4 x 12 x 96");
    fireEvent.change(screen.getByLabelText("Stock for Oak"), { target: { value: "" } });
    expect(p.onDefault).toHaveBeenLastCalledWith("Oak", null);
    // Add a 5 x 5 sheet.
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "5 x 5 birch, 1/2" } });
    fireEvent.change(screen.getByLabelText("Width"), { target: { value: "60" } });
    fireEvent.change(screen.getByLabelText("Length"), { target: { value: "5'" } });
    fireEvent.change(screen.getByLabelText("Thickness"), { target: { value: "1/2" } });
    fireEvent.click(screen.getByRole("button", { name: "Add stock" }));
    expect(p.onDefine).toHaveBeenLastCalledWith({ name: "5 x 5 birch, 1/2", kind: "sheet", width: 60 * IN, length: 60 * IN, thickness: 0.5 * IN, nominal: null, grain: "length" }, null);
    // Edit the board: the form fills in with units; saving names the one replaced.
    fireEvent.click(screen.getByRole("button", { name: "Edit 4/4 x 12 x 96" }));
    expect((screen.getByLabelText("Width") as HTMLInputElement).value).toBe("12 in");
    expect((screen.getByLabelText("Nominal") as HTMLInputElement).value).toBe("4/4");
    fireEvent.change(screen.getByLabelText("Length"), { target: { value: "10'" } });
    fireEvent.click(screen.getByRole("button", { name: "Save stock" }));
    expect(p.onDefine).toHaveBeenLastCalledWith(expect.objectContaining({ name: "4/4 x 12 x 96", kind: "board", length: 120 * IN, nominal: "4/4" }), "4/4 x 12 x 96");
    fireEvent.click(screen.getByRole("button", { name: "Delete 4 x 8 sheet, 1/4" }));
    expect(p.onDelete).toHaveBeenCalledWith("4 x 8 sheet, 1/4");
    fireEvent.click(screen.getByRole("button", { name: "Built-in stock" }));
    expect(p.onReset).toHaveBeenCalled();
  });

  it("refuses stock nobody can cut from, and says why", () => {
    const p = dprops();
    render(<StockDialog {...p} />);
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Odd" } });
    fireEvent.change(screen.getByLabelText("Thickness"), { target: { value: "60" } });
    fireEvent.click(screen.getByRole("button", { name: "Add stock" }));
    expect(p.onDefine).not.toHaveBeenCalled();
    expect(screen.getByText(/thicker than it is wide/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Width"), { target: { value: "wide" } });
    fireEvent.click(screen.getByRole("button", { name: "Add stock" }));
    expect(screen.getByText(/Width:/)).toBeTruthy();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(p.onClose).toHaveBeenCalled();
  });
});

describe("Cut list pane with nesting", () => {
  function cprops(over: Partial<CutListPaneProps> = {}): CutListPaneProps {
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

  it("the Nesting button opens and closes the section under the list", () => {
    const onNest = vi.fn();
    const { rerender } = render(<CutListPane {...cprops({ onNest, nestOpen: false })} />);
    const b = screen.getByRole("button", { name: "Nesting ›" });
    expect(b.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(b);
    expect(onNest).toHaveBeenCalled();
    rerender(<CutListPane {...cprops({ onNest, nestOpen: true, nesting: <NestingSection {...props()} /> })} />);
    expect(screen.getByRole("button", { name: "Nesting ‹" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByTestId("nesting")).toBeTruthy();
  });
});
