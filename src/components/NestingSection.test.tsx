import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { gluedNesting, smallNesting } from "../nesting/fixture";
import { NestingSection, type NestingSectionProps } from "./NestingSection";
import { StockDialog, type StockDialogProps } from "./StockDialog";
import { CutListPane, type CutListPaneProps } from "./CutListPane";
import { bookshelf } from "../cutlist/fixture";
import { parseLength } from "../lib/units";

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

  it("#49: the joint loss takes a typed length, 0 to 1/2 in, and keeps the other settings", () => {
    const p = props();
    render(<NestingSection {...p} />);
    const joint = screen.getByLabelText("Joint loss") as HTMLInputElement;
    // Absent from the document's settings: the 1/8 in default.
    expect(joint.value).toBe("1/8 in");
    fireEvent.change(joint, { target: { value: "1/16" } });
    fireEvent.keyDown(joint, { key: "Enter" });
    expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: IN / 8, trim: 0, algorithm: "guillotine", jointLoss: IN / 16 });
    fireEvent.change(joint, { target: { value: "1 in" } });
    fireEvent.keyDown(joint, { key: "Enter" });
    expect(p.onSettings).toHaveBeenCalledTimes(1);
    expect(joint.closest("label")!.getAttribute("title")).toContain("0 to 1/2 in");
  });

  it("#57: the minimum strip is empty (auto) until set, takes a typed length, 0 to 12 in, and clears back to auto", () => {
    const p = props();
    const { rerender } = render(<NestingSection {...p} />);
    const min = screen.getByLabelText("Minimum strip") as HTMLInputElement;
    // Absent from the document's settings: empty, the #56 rule.
    expect(min.value).toBe("");
    expect(min.placeholder).toBe("auto");
    expect(min.closest("label")!.getAttribute("title")).toContain("1 1/2 in, or a quarter of the board");
    fireEvent.change(min, { target: { value: "2" } });
    fireEvent.keyDown(min, { key: "Enter" });
    expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: IN / 8, trim: 0, algorithm: "guillotine", minStrip: 2 * IN });
    // Out of range: refused, nothing sent.
    fireEvent.change(min, { target: { value: "13" } });
    fireEvent.keyDown(min, { key: "Enter" });
    expect(p.onSettings).toHaveBeenCalledTimes(1);
    expect(min.closest("label")!.getAttribute("title")).toContain("0 to 12 in");
    // Set in the document: shown; cleared: the setting goes, not a 0.
    const n = smallNesting();
    n.settings = { ...n.settings, minStrip: 2 * IN };
    rerender(<NestingSection {...p} nesting={n} />);
    expect(min.value).toBe("2 in");
    fireEvent.change(min, { target: { value: "" } });
    fireEvent.keyDown(min, { key: "Enter" });
    expect(p.onSettings).toHaveBeenCalledTimes(2);
    const last = (p.onSettings as unknown as { mock: { calls: [Record<string, unknown>][] } }).mock.calls[1][0];
    expect(last).toEqual({ kerf: IN / 8, trim: 0, algorithm: "guillotine" });
    expect("minStrip" in last).toBe(false);
    // 0 is a value (never even out), not auto.
    fireEvent.change(min, { target: { value: "0" } });
    fireEvent.keyDown(min, { key: "Enter" });
    expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: IN / 8, trim: 0, algorithm: "guillotine", minStrip: 0 });
  });

  it("#57: a pause on an emptied Minimum strip field clears nothing; Enter or leaving the field does", () => {
    vi.useFakeTimers();
    try {
      const n = smallNesting();
      n.settings = { ...n.settings, minStrip: 2 * IN };
      const p = props({ nesting: n });
      render(<NestingSection {...p} />);
      const min = screen.getByLabelText("Minimum strip") as HTMLInputElement;
      // Backspaced to empty on the way to typing another length: the pause sends nothing.
      fireEvent.change(min, { target: { value: "" } });
      act(() => vi.advanceTimersByTime(2000));
      expect(p.onSettings).not.toHaveBeenCalled();
      // A length typed then pauses commit as before.
      fireEvent.change(min, { target: { value: "3" } });
      act(() => vi.advanceTimersByTime(800));
      expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: IN / 8, trim: 0, algorithm: "guillotine", minStrip: 3 * IN });
      // Emptied and left: cleared.
      fireEvent.change(min, { target: { value: "" } });
      fireEvent.blur(min);
      expect(p.onSettings).toHaveBeenCalledTimes(2);
      expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: IN / 8, trim: 0, algorithm: "guillotine" });
    } finally {
      vi.useRealTimers();
    }
  });

  it("#57: the minimum strip reads millimetres in a metric document and takes 12 in written any way", () => {
    const n = smallNesting();
    n.settings = { ...n.settings, minStrip: 0.038 };
    const p = props({ nesting: n, units: "mm", rounding: "half_mm" });
    render(<NestingSection {...p} />);
    const min = screen.getByLabelText("Minimum strip") as HTMLInputElement;
    expect(min.value).toBe("38 mm");
    // 12 in exactly, as 304.8 mm, 1 ft and 12 in: each taken, none refused as past 12 in.
    for (const [typed, m] of [["304.8", 0.3048], ["1'", 0.3048], ["12in", 12 * IN]] as const) {
      fireEvent.change(min, { target: { value: typed } });
      fireEvent.keyDown(min, { key: "Enter" });
      expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: IN / 8, trim: 0, algorithm: "guillotine", minStrip: m });
    }
    expect(p.onSettings).toHaveBeenCalledTimes(3);
    fireEvent.change(min, { target: { value: "304.9" } });
    fireEvent.keyDown(min, { key: "Enter" });
    expect(p.onSettings).toHaveBeenCalledTimes(3);
    expect(min.closest("label")!.getAttribute("title")).toContain("0 to 304.8 mm");
  });

  it("#57 (lane 56): the minimum strip's top is exactly 0.3048 m, the float above it refused", () => {
    // The neighbouring doubles of 0.3048, by their bits.
    const step = (x: number, by: 1n | -1n) => {
      const b = new BigInt64Array(new Float64Array([x]).buffer);
      b[0] += by;
      return new Float64Array(b.buffer)[0];
    };
    const above = step(0.3048, 1n);
    const below = step(0.3048, -1n);
    // Typed in metres, each reads as exactly that double.
    const typed = (m: number) => `${m} m`;
    for (const m of [0.3048, above, below]) {
      const r = parseLength(typed(m), "frac-in");
      expect(r.ok && r.metres === m).toBe(true);
    }
    expect(above).toBe(0.30480000000000007);
    const p = props();
    render(<NestingSection {...p} />);
    const min = screen.getByLabelText("Minimum strip") as HTMLInputElement;
    // 0.3048 m and the double under it are taken (a top moved one step down refuses 0.3048).
    for (const m of [0.3048, below]) {
      fireEvent.change(min, { target: { value: typed(m) } });
      fireEvent.keyDown(min, { key: "Enter" });
      expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: IN / 8, trim: 0, algorithm: "guillotine", minStrip: m });
    }
    expect(p.onSettings).toHaveBeenCalledTimes(2);
    // The double above it is refused (a top moved one step up takes it), and nothing is sent.
    fireEvent.change(min, { target: { value: typed(above) } });
    fireEvent.keyDown(min, { key: "Enter" });
    expect(p.onSettings).toHaveBeenCalledTimes(2);
    expect(min.closest("label")!.getAttribute("title")).toContain("Minimum strip: 0 to 12 in.");
  });

  it("#57 (lane 56): a minimum strip set to 0 shows 0 in the document's units, not auto", () => {
    for (const [units, rounding, shown] of [
      ["frac-in", "sixteenth", "0 in"],
      ["mm", "half_mm", "0 mm"],
    ] as const) {
      const n = smallNesting();
      n.settings = { ...n.settings, minStrip: 0 };
      const p = props({ nesting: n, units, rounding });
      render(<NestingSection {...p} />);
      const min = screen.getByLabelText("Minimum strip") as HTMLInputElement;
      expect(min.value).toBe(shown);
      // Shown as set, Enter on it sends nothing; emptied, it clears back to auto.
      fireEvent.keyDown(min, { key: "Enter" });
      expect(p.onSettings).not.toHaveBeenCalled();
      fireEvent.change(min, { target: { value: "" } });
      fireEvent.keyDown(min, { key: "Enter" });
      expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: IN / 8, trim: 0, algorithm: "guillotine" });
      cleanup();
    }
  });

  it("#89: the 1/8 in kerf and joint loss read 3.18 mm, and Enter on them or leaving them keeps exactly 1/8 in", () => {
    const p = props({ units: "mm", rounding: "half_mm" });
    render(<NestingSection {...p} />);
    for (const label of ["Kerf", "Joint loss"]) {
      const f = screen.getByLabelText(label) as HTMLInputElement;
      expect(f.value).toBe("3.18 mm");
      fireEvent.keyDown(f, { key: "Enter" });
      fireEvent.blur(f);
    }
    // 3.18 mm read back would be 0.005 mm wider than the 1/8 in the document holds.
    expect(p.onSettings).not.toHaveBeenCalled();
  });

  it("#92: Enter and blur keep the exact kerf when the decimal display rounds up", () => {
    const nesting = smallNesting();
    nesting.settings.kerf = (3 / 16) * IN;
    const p = props({ nesting, units: "in", rounding: { r: "sixteenth", style: "decimal" } });
    render(<NestingSection {...p} />);
    const kerf = screen.getByLabelText("Kerf") as HTMLInputElement;
    expect(kerf.value).toBe("0.188 in");
    fireEvent.keyDown(kerf, { key: "Enter" });
    fireEvent.blur(kerf);
    expect(p.onSettings).not.toHaveBeenCalled();
  });

  it("#92: a kerf and joint loss reading 1/32 in or 0.188 in keep their exact length on Enter or leaving the field", () => {
    for (const [units, rounding, length, shown] of [
      ["frac-in", "sixteenth", (3 / 128) * IN, "1/32 in"],
      ["in", { r: "sixteenth", style: "decimal" }, (3 / 16) * IN, "0.188 in"],
    ] as const) {
      const n = smallNesting();
      n.settings = { ...n.settings, kerf: length, jointLoss: length };
      const p = props({ nesting: n, units, rounding });
      render(<NestingSection {...p} />);
      for (const label of ["Kerf", "Joint loss"]) {
        const f = screen.getByLabelText(label) as HTMLInputElement;
        expect(f.value).toBe(shown);
        fireEvent.keyDown(f, { key: "Enter" });
        fireEvent.blur(f);
      }
      // Read back, 1/32 in is 1/128 in wider and 0.188 in 0.0005 in wider than what is held.
      expect(p.onSettings).not.toHaveBeenCalled();
      cleanup();
    }
  });

  it("#93: a committed field that reads the same still gets its unit back", () => {
    // A typed value that rounds to the text already shown leaves the raw text in the box:
    // the `shown` string does not change, so the effect that resets it never runs. #93.
    // Millimetres: the built-in 1/8 in (3.175 mm) reads "3.18 mm"; typing 3.18 reads the
    // same, so the committed value must still be shown with its unit.
    {
      const p = props({ units: "mm", rounding: "half_mm" });
      render(<NestingSection {...p} />);
      const kerf = screen.getByLabelText("Kerf") as HTMLInputElement;
      expect(kerf.value).toBe("3.18 mm");
      fireEvent.change(kerf, { target: { value: "3.18" } });
      fireEvent.keyDown(kerf, { key: "Enter" });
      expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: 0.00318, trim: 0, algorithm: "guillotine" });
      expect(kerf.value).toBe("3.18 mm");
      cleanup();
    }
    // Fractional inches: typing 0.13 in rounds to 1/8 in, the text already shown.
    {
      const p = props();
      render(<NestingSection {...p} />);
      const kerf = screen.getByLabelText("Kerf") as HTMLInputElement;
      expect(kerf.value).toBe("1/8 in");
      fireEvent.change(kerf, { target: { value: "0.13" } });
      fireEvent.keyDown(kerf, { key: "Enter" });
      expect(kerf.value).toBe("1/8 in");
      cleanup();
    }
  });

  // #93, more cases: the settings as the app holds them after a commit, Undo or Redo.
  const at93 = (s: Partial<ReturnType<typeof smallNesting>["settings"]>) => {
    const n = smallNesting();
    return { ...n, settings: { ...n.settings, ...s } };
  };

  it("#93: leaving the field commits a length that reads the same, and Undo and Redo keep the unit", () => {
    const p = props({ units: "mm", rounding: "half_mm" });
    const { rerender } = render(<NestingSection {...p} />);
    const kerf = screen.getByLabelText("Kerf") as HTMLInputElement;
    fireEvent.change(kerf, { target: { value: "3.18" } });
    fireEvent.blur(kerf);
    expect(p.onSettings).toHaveBeenCalledTimes(1);
    expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: 0.00318, trim: 0, algorithm: "guillotine" });
    expect(kerf.value).toBe("3.18 mm");
    // The app applies it; then Undo (back to 1/8 in) and Redo: the reading never changes.
    rerender(<NestingSection {...p} nesting={at93({ kerf: 0.00318 })} />);
    expect(kerf.value).toBe("3.18 mm");
    rerender(<NestingSection {...p} nesting={at93({})} />);
    expect(kerf.value).toBe("3.18 mm");
    rerender(<NestingSection {...p} nesting={at93({ kerf: 0.00318 })} />);
    expect(kerf.value).toBe("3.18 mm");
    // Enter and leaving the field again commit nothing more.
    fireEvent.keyDown(kerf, { key: "Enter" });
    fireEvent.blur(kerf);
    expect(p.onSettings).toHaveBeenCalledTimes(1);
  });

  it("#93: Trim, Joint loss and Minimum strip get their unit back too, and so do decimal inches", () => {
    {
      const p = props({ units: "mm", rounding: "half_mm", nesting: at93({ minStrip: 0.0381 }) });
      render(<NestingSection {...p} />);
      const trim = screen.getByLabelText("Trim") as HTMLInputElement;
      expect(trim.value).toBe("0 mm");
      fireEvent.change(trim, { target: { value: "0" } });
      fireEvent.keyDown(trim, { key: "Enter" });
      expect(trim.value).toBe("0 mm");
      expect(p.onSettings).not.toHaveBeenCalled();
      const loss = screen.getByLabelText("Joint loss") as HTMLInputElement;
      fireEvent.change(loss, { target: { value: "3.18" } });
      fireEvent.keyDown(loss, { key: "Enter" });
      expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: IN / 8, trim: 0, jointLoss: 0.00318, minStrip: 0.0381, algorithm: "guillotine" });
      expect(loss.value).toBe("3.18 mm");
      const min = screen.getByLabelText("Minimum strip") as HTMLInputElement;
      expect(min.value).toBe("38.1 mm");
      fireEvent.change(min, { target: { value: "38.1" } });
      fireEvent.keyDown(min, { key: "Enter" });
      expect(min.value).toBe("38.1 mm");
      cleanup();
    }
    {
      const p = props({ units: "in", rounding: { r: "sixteenth", style: "decimal" } });
      render(<NestingSection {...p} />);
      const kerf = screen.getByLabelText("Kerf") as HTMLInputElement;
      expect(kerf.value).toBe("0.125 in");
      fireEvent.change(kerf, { target: { value: "0.1251" } });
      fireEvent.keyDown(kerf, { key: "Enter" });
      expect(p.onSettings).toHaveBeenCalledTimes(1);
      expect(kerf.value).toBe("0.125 in");
      cleanup();
    }
  });

  it("#93: the pause commits a length that reads the same but leaves the typed text until Enter or leaving the field", () => {
    vi.useFakeTimers();
    try {
      const p = props({ units: "mm", rounding: "half_mm" });
      const { rerender } = render(<NestingSection {...p} />);
      const kerf = screen.getByLabelText("Kerf") as HTMLInputElement;
      fireEvent.change(kerf, { target: { value: "3.18" } });
      vi.advanceTimersByTime(800);
      expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: 0.00318, trim: 0, algorithm: "guillotine" });
      rerender(<NestingSection {...p} nesting={at93({ kerf: 0.00318 })} />);
      // Still what was typed: the person may be going on to type 3.185.
      expect(kerf.value).toBe("3.18");
      fireEvent.blur(kerf);
      expect(kerf.value).toBe("3.18 mm");
      expect(p.onSettings).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("#93: a length that reads differently keeps the typed text until the new value arrives, then reads it", () => {
    vi.useFakeTimers();
    try {
      const p = props({ units: "mm", rounding: "half_mm" });
      const { rerender } = render(<NestingSection {...p} />);
      const kerf = screen.getByLabelText("Kerf") as HTMLInputElement;
      fireEvent.change(kerf, { target: { value: "5" } });
      fireEvent.keyDown(kerf, { key: "Enter" });
      expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: 0.005, trim: 0, algorithm: "guillotine" });
      // Not the old 3.18 mm while the app works it out.
      expect(kerf.value).toBe("5");
      rerender(<NestingSection {...p} nesting={at93({ kerf: 0.005 })} />);
      expect(kerf.value).toBe("5 mm");
      // The same by the pause.
      fireEvent.change(kerf, { target: { value: "4" } });
      vi.advanceTimersByTime(800);
      expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: 0.004, trim: 0, algorithm: "guillotine" });
      expect(kerf.value).toBe("4");
      rerender(<NestingSection {...p} nesting={at93({ kerf: 0.004 })} />);
      expect(kerf.value).toBe("4 mm");
    } finally {
      vi.useRealTimers();
    }
  });

  it("#93: a length that is refused stays as typed, with the reason", () => {
    const p = props({ units: "mm", rounding: "half_mm" });
    render(<NestingSection {...p} />);
    const kerf = screen.getByLabelText("Kerf") as HTMLInputElement;
    for (const typed of ["abc", "2 in"]) {
      fireEvent.change(kerf, { target: { value: typed } });
      fireEvent.keyDown(kerf, { key: "Enter" });
      expect(kerf.value).toBe(typed);
      expect(kerf.closest("label")!.className).toBe("err");
      fireEvent.blur(kerf);
      expect(kerf.value).toBe(typed);
    }
    expect(p.onSettings).not.toHaveBeenCalled();
  });

  it("#93: a stored kerf over the field's top, typed as it reads, is refused and stays as typed", () => {
    // A file can hold a kerf the field would refuse: 0.6 in, over the 1/2 in top.
    const p = props({ units: "mm", rounding: "half_mm", nesting: at93({ kerf: 0.6 * IN }) });
    render(<NestingSection {...p} />);
    const kerf = screen.getByLabelText("Kerf") as HTMLInputElement;
    expect(kerf.value).toBe("15.24 mm");
    fireEvent.change(kerf, { target: { value: "15.24" } });
    fireEvent.keyDown(kerf, { key: "Enter" });
    expect(kerf.value).toBe("15.24");
    expect(kerf.closest("label")!.className).toBe("err");
    expect(kerf.closest("label")!.title).toBe("Kerf: 0 to 12.7 mm.");
    expect(p.onSettings).not.toHaveBeenCalled();
  });

  it("#49: a glued part lists its glue-up, each strip at its own width, and the model lights every strip", () => {
    const n = smallNesting();
    const piece = n.pieces[0];
    // A 15 x 34 1/2 in shelf glued from the 12 in board: 12 + 3 1/8 (1/8 in joint loss).
    const strip = (k: number, y: number, w: number) => ({ ...piece.placements[0], tag: `1a.${k}`, y: y * IN, dy: w * IN, size: [0.75 * IN, 15 * IN, 34.5 * IN] as [number, number, number], strip: { n: k, of: 2 } });
    piece.placements = [strip(1, 0, 12), { ...strip(2, 0, 3.125), x: 34.625 * IN }, piece.placements[1]];
    n.glue_ups = [{ row: 0, copy: 1, tag: "1a", name: "Shelf", item: piece.placements[0].item, stock: "4/4 x 12 x 96", widths: [12 * IN, 3.125 * IN] }];
    const p = props({ nesting: n, lit: "1a" });
    render(<NestingSection {...p} />);
    expect(screen.getByTestId("nest-glued").textContent).toContain("1a Shelf: glued up from 2 strips of 4/4 x 12 x 96, 12 + 3 1/8 in");
    const parts = within(screen.getAllByRole("figure")[0]).getAllByRole("listitem").map((li) => li.textContent);
    expect(parts).toContain("1a.1 Shelf, strip 1 of 2 12 × 34 1/2");
    expect(parts).toContain("1a.2 Shelf, strip 2 of 2 3 1/8 × 34 1/2");
    // The model's part lights both strips, and not the other shelf.
    const lit = [...document.querySelectorAll(".nest-diagram .part.on")].map((g) => g.getAttribute("data-tag"));
    expect(lit.sort()).toEqual(["1a.1", "1a.2"]);
    // The footer names the joint loss when something is glued up.
    expect(screen.getByTestId("nest-note").textContent).toContain("joint loss 1/8 in");
  });

  // Lane 30, from the #33 review: the field's 700 ms pause timer kept the value, the shown
  // text and the settings of the render it was set in.
  it("Enter commits a typed kerf once: the pause after the typing does not commit it again", () => {
    vi.useFakeTimers();
    const p = props();
    const { rerender } = render(<NestingSection {...p} />);
    const kerf = screen.getByLabelText("Kerf") as HTMLInputElement;
    // Typed the way the field shows lengths, so the shown text will not change it back.
    fireEvent.change(kerf, { target: { value: "3/16 in" } });
    fireEvent.keyDown(kerf, { key: "Enter" });
    expect(p.onSettings).toHaveBeenCalledTimes(1);
    // The app applies it and the field shows it.
    const n = smallNesting();
    rerender(<NestingSection {...p} nesting={{ ...n, settings: { ...n.settings, kerf: (3 / 16) * IN } }} />);
    expect(kerf.value).toBe("3/16 in");
    vi.advanceTimersByTime(800);
    expect(p.onSettings).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });

  it("a value changed from outside while a typed one waits is kept: the timer neither commits again nor undoes the other field", () => {
    vi.useFakeTimers();
    const p = props();
    const { rerender } = render(<NestingSection {...p} />);
    const kerf = screen.getByLabelText("Kerf") as HTMLInputElement;
    fireEvent.change(kerf, { target: { value: "3/16 in" } });
    vi.advanceTimersByTime(300);
    // A plugin (or Redo) sets the kerf to what was typed and the trim to 1/4 in.
    const n = smallNesting();
    rerender(<NestingSection {...p} nesting={{ ...n, settings: { ...n.settings, kerf: (3 / 16) * IN, trim: IN / 4 } }} />);
    expect(kerf.value).toBe("3/16 in");
    expect((screen.getByLabelText("Trim") as HTMLInputElement).value).toBe("1/4 in");
    vi.advanceTimersByTime(800);
    // Nothing to commit: the kerf already is 3/16, and the trim must not go back to 0.
    expect(p.onSettings).not.toHaveBeenCalled();
    // Typing after that commits against the settings as they are now.
    fireEvent.change(kerf, { target: { value: "1/16" } });
    vi.advanceTimersByTime(800);
    expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: IN / 16, trim: IN / 4, algorithm: "guillotine" });
    expect(p.onSettings).toHaveBeenCalledTimes(1);
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
    fireEvent.change(screen.getByLabelText("Actual thickness"), { target: { value: "1/2" } });
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

  it("#58: Built-in stock's tooltip says what it puts back in the document's units", () => {
    const title = (over: Partial<StockDialogProps>) => {
      const { unmount } = render(<StockDialog {...dprops(over)} />);
      const t = screen.getByRole("button", { name: "Built-in stock" }).getAttribute("title");
      unmount();
      return t;
    };
    expect(title({})).toBe("Back to the built-in sheets and boards: kerf 1/8 in, no trim, joint loss 1/8 in, minimum strip auto, table saw");
    expect(title({ rounding: "half_mm", units: "mm" })).toBe("Back to the built-in sheets and boards: kerf 3.18 mm, no trim, joint loss 3.18 mm, minimum strip auto, table saw");
    expect(title({ rounding: { r: "sixteenth", style: "decimal" }, units: "in" })).toBe("Back to the built-in sheets and boards: kerf 0.125 in, no trim, joint loss 0.125 in, minimum strip auto, table saw");
  });

  it("fills the actual thickness from the nominal until it is typed (lane 15)", () => {
    const p = dprops();
    render(<StockDialog {...p} />);
    const actual = () => (screen.getByLabelText("Actual thickness") as HTMLInputElement).value;
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Ply 3/4" } });
    fireEvent.change(screen.getByLabelText("Nominal"), { target: { value: "3/4" } });
    expect(actual()).toBe("23/32 in");
    fireEvent.change(screen.getByLabelText("Kind"), { target: { value: "board" } });
    fireEvent.change(screen.getByLabelText("Nominal"), { target: { value: "8/4" } });
    expect(actual()).toBe("1 3/4 in");
    fireEvent.change(screen.getByLabelText("Nominal"), { target: { value: "5/4" } });
    expect(actual()).toBe("1 1/16 in");
    // An override sticks: the nominal no longer changes it.
    fireEvent.change(screen.getByLabelText("Actual thickness"), { target: { value: "1-1/8\"" } });
    fireEvent.change(screen.getByLabelText("Nominal"), { target: { value: "6/4" } });
    expect(actual()).toBe('1-1/8"');
    fireEvent.click(screen.getByRole("button", { name: "Add stock" }));
    expect(p.onDefine).toHaveBeenLastCalledWith(expect.objectContaining({ name: "Ply 3/4", kind: "board", thickness: 1.125 * IN, nominal: "6/4" }), null);
    // A sheet keeps its nominal too.
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Ply half" } });
    fireEvent.change(screen.getByLabelText("Nominal"), { target: { value: "1/2" } });
    expect(actual()).toBe("15/32 in");
    fireEvent.click(screen.getByRole("button", { name: "Add stock" }));
    expect(p.onDefine).toHaveBeenLastCalledWith(expect.objectContaining({ kind: "sheet", thickness: (15 / 32) * IN, nominal: "1/2" }), null);
  });

  // Lane 15 verifier: the list rounded actual thicknesses to the cut list's 1/16 in, so
  // 23/32, 15/32 and 7/32 plywood read "3/4 in actual", "1/2 in actual", "1/4 in actual".
  it("lists each stock's actual thickness at the saw's precision, not the cut list's rounding", () => {
    const sheet = (nominal: string, t: number) => ({ name: `4 x 8 ply, ${nominal}`, kind: "sheet" as const, width: 48 * IN, length: 96 * IN, thickness: t * IN, nominal, grain: "length" as const });
    const library = { ...smallNesting().library, stocks: [sheet("3/4", 23 / 32), sheet("1/2", 15 / 32), sheet("1/4", 7 / 32)] };
    render(<StockDialog {...dprops({ library })} />);
    const list = screen.getByRole("table", { name: "Stock library" }).textContent ?? "";
    expect(list).toContain("sheet 3/4 nominal · 48 × 96 × 23/32 in actual");
    expect(list).toContain("sheet 1/2 nominal · 48 × 96 × 15/32 in actual");
    expect(list).toContain("sheet 1/4 nominal · 48 × 96 × 7/32 in actual");
    expect(list).not.toContain("× 3/4 in actual");
    cleanup();
    render(<StockDialog {...dprops({ library, rounding: "half_mm" })} />);
    expect(screen.getByRole("table", { name: "Stock library" }).textContent).toContain("1219 × 2438.5 × 18.26 mm actual");
  });

  it("refuses stock nobody can cut from, and says why", () => {
    const p = dprops();
    render(<StockDialog {...p} />);
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Odd" } });
    fireEvent.change(screen.getByLabelText("Actual thickness"), { target: { value: "60" } });
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

  it("#56: a glued row says what it is glued from while the nesting is there, and reads as before without it", () => {
    // The bookshelf's shelves (row 1), all four glued from the 12 in board.
    const list = bookshelf();
    const nodes = [3, 4, 5, 6];
    list.rows[1].occurrences = nodes.map((node) => ({ item: { node }, visible: true, handed: false }) as never);
    const n = smallNesting();
    n.glue_ups = nodes.map((node, k) => ({ row: 1, copy: k + 1, tag: `2${"abcd"[k]}`, name: "Shelf", item: { node }, stock: "4/4 x 12 x 96", widths: [12 * IN, 3.125 * IN] }));
    const notes = () => screen.getByRole("row", { name: /^Shelf/ }).querySelector("td.part")!.textContent;
    const { rerender } = render(<CutListPane {...cprops({ list, nestOpen: true, nested: n })} />);
    expect(notes()).toContain("glued from 2 (4/4 x 12 x 96)");
    // The other rows have no such note.
    expect(screen.getByRole("row", { name: /^Side/ }).textContent).not.toContain("glued");
    // Without the nesting (its section closed), the row reads as before.
    rerender(<CutListPane {...cprops({ list, nestOpen: false, nested: null })} />);
    expect(notes()).not.toContain("glued");
  });

  it("#56: a board's caption and label count a glued part once, with its strips", () => {
    render(<NestingSection {...props({ nesting: gluedNesting() })} />);
    const fig = screen.getAllByRole("figure")[0];
    expect(fig.querySelector("summary")!.textContent).toContain("4 cuts, 2 parts in 3 pieces");
    expect(within(fig).getByRole("img").getAttribute("aria-label")).toContain(": 2 parts in 3 pieces, ");
    // The sheet, with no strips, reads as before.
    expect(screen.getAllByRole("figure")[1].querySelector("summary")!.textContent).toContain("2 cuts, 1 part");
  });
  const kerfOf = (kerf: number) => {
    const n = smallNesting();
    return { ...n, settings: { ...n.settings, kerf } };
  };
  // #98: text typed into a focused length field is not replaced when the reading of the
  // stored value changes meanwhile; the new reading waits for Enter, leaving or Escape.
  it("#98: the pause stores what was typed, and the next keys land in the typed text", () => {
    vi.useFakeTimers();
    try {
      const p = props({ units: "mm", rounding: "half_mm" });
      const { rerender } = render(<NestingSection {...p} />);
      const kerf = screen.getByLabelText("Kerf") as HTMLInputElement;
      act(() => kerf.focus());
      fireEvent.change(kerf, { target: { value: "3.1" } });
      act(() => void vi.advanceTimersByTime(800));
      expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: 3.1 * 0.001, trim: 0, algorithm: "guillotine" });
      // The app applies the pause's length; the field is still being typed in.
      rerender(<NestingSection {...p} nesting={kerfOf(0.0031)} />);
      expect(kerf.value).toBe("3.1");
      fireEvent.change(kerf, { target: { value: "3.15" } });
      expect(kerf.value).toBe("3.15");
      fireEvent.keyDown(kerf, { key: "Enter" });
      expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: 3.15 * 0.001, trim: 0, algorithm: "guillotine" });
      rerender(<NestingSection {...p} nesting={kerfOf(0.00315)} />);
      expect(kerf.value).toBe("3.15 mm");
      expect(screen.queryByTitle(/mm5/)).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("#98: a late refresh that changes the reading leaves the typed text, and Enter stores it", () => {
    vi.useFakeTimers();
    try {
      const p = props({ units: "mm", rounding: "half_mm" });
      const { rerender } = render(<NestingSection {...p} />);
      const kerf = screen.getByLabelText("Kerf") as HTMLInputElement;
      act(() => kerf.focus());
      fireEvent.change(kerf, { target: { value: "5" } });
      // Undo from elsewhere lands before the pause: the stored kerf now reads 2 mm.
      rerender(<NestingSection {...p} nesting={kerfOf(0.002)} />);
      expect(kerf.value).toBe("5");
      fireEvent.keyDown(kerf, { key: "Enter" });
      expect(p.onSettings).toHaveBeenLastCalledWith({ kerf: 0.005, trim: 0, algorithm: "guillotine" });
      rerender(<NestingSection {...p} nesting={kerfOf(0.005)} />);
      expect(kerf.value).toBe("5 mm");
    } finally {
      vi.useRealTimers();
    }
  });

  it("#98: the new reading shows after leaving the field or Escape", () => {
    const p = props({ units: "mm", rounding: "half_mm" });
    const { rerender } = render(<NestingSection {...p} />);
    const kerf = screen.getByLabelText("Kerf") as HTMLInputElement;
    // Leaving: a reading that changed meanwhile is shown once the typed one is committed.
    act(() => kerf.focus());
    fireEvent.change(kerf, { target: { value: "5" } });
    rerender(<NestingSection {...p} nesting={kerfOf(0.002)} />);
    expect(kerf.value).toBe("5");
    fireEvent.blur(kerf);
    rerender(<NestingSection {...p} nesting={kerfOf(0.005)} />);
    expect(kerf.value).toBe("5 mm");
    // Escape gives the typed text up for the reading now stored.
    act(() => kerf.focus());
    fireEvent.change(kerf, { target: { value: "7" } });
    rerender(<NestingSection {...p} nesting={kerfOf(0.002)} />);
    expect(kerf.value).toBe("7");
    fireEvent.keyDown(kerf, { key: "Escape" });
    expect(kerf.value).toBe("2 mm");
  });
});
