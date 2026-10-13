import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SheetRow } from "../kernel/sheetTypes";
import { newSetup } from "./paper";
import { SheetsPane, setupOf, viewportScaleFor, type SheetsPaneProps } from "./SheetsPane";
import { ModeSwitch, SheetTabs } from "./SheetTabs";

afterEach(cleanup);

const IN = 0.0254;

function sheet(): SheetRow {
  return {
    id: 1,
    sheet: {
      ...newSetup("Bookshelf", false),
      items: [
        { id: 1, kind: "viewport", rect: [0.5 * IN, 0.5 * IN, 4 * IN, 7 * IN], source: { from: "standard", view: "front" }, scale: { kind: "ratio", paper: 1, model: 12 }, caption: "FRONT ELEVATION 1:12" },
        { id: 2, kind: "dimension", viewport: 1, dim: "horizontal", points: [[0, 0, 0], [0.9144, 0, 0]], offset: -0.3 * IN },
        { id: 3, kind: "table", rect: [5 * IN, 3 * IN, 5 * IN, 2 * IN], columns: ["part", "qty"], title: "CUT LIST" },
      ],
      next_item: 4,
    },
  };
}

function props(over: Partial<SheetsPaneProps> = {}): SheetsPaneProps {
  return {
    sheets: [sheet()],
    active: 1,
    render: null,
    selection: [],
    scenes: [{ name: "Front", eye: [0, -3, 1], target: [0, 0, 1], up: [0, 0, 1], parallel: true }],
    tool: "select",
    newSource: { from: "standard", view: "front" },
    onNewSource: vi.fn(),
    dimKind: "horizontal",
    onDimKind: vi.fn(),
    onSetup: vi.fn(),
    onSetItem: vi.fn(),
    onDeleteItems: vi.fn(),
    onOrder: vi.fn(),
    onFitScale: vi.fn(),
    onCurrentView: vi.fn(),
    onExport: vi.fn(),
    exports: ["svg"],
    ...over,
  };
}

describe("Sheets pane", () => {
  it("changes the paper as one edit per change", () => {
    const p = props();
    render(<SheetsPane {...p} />);
    fireEvent.change(screen.getByRole("combobox", { name: "Paper size" }), { target: { value: "tabloid" } });
    expect(p.onSetup).toHaveBeenLastCalledWith(1, { ...setupOf(sheet()), paper: { kind: "tabloid" } });
    fireEvent.click(screen.getByRole("radio", { name: "Portrait" }));
    // No new state came back in between, so the second edit builds on the first.
    expect(p.onSetup).toHaveBeenLastCalledWith(1, { ...setupOf(sheet()), paper: { kind: "tabloid" }, landscape: false });
    // A margin typed as a fraction, committed on Enter (not per letter).
    const top = screen.getByRole("textbox", { name: "Top margin" });
    fireEvent.change(top, { target: { value: "3/4" } });
    expect(p.onSetup).toHaveBeenCalledTimes(2);
    fireEvent.keyDown(top, { key: "Enter" });
    const last = (p.onSetup as ReturnType<typeof vi.fn>).mock.calls.at(-1)![1];
    expect(last.margins[0]).toBeCloseTo(0.75 * IN, 12);
    // The title block: a field left with Esc changes nothing.
    const title = screen.getByRole("textbox", { name: /^Title$/ });
    fireEvent.change(title, { target: { value: "Shelf" } });
    fireEvent.keyDown(title, { key: "Escape" });
    expect(p.onSetup).toHaveBeenCalledTimes(3);
    fireEvent.change(title, { target: { value: "Shelf" } });
    fireEvent.blur(title);
    expect((p.onSetup as ReturnType<typeof vi.fn>).mock.calls.at(-1)![1].title).toEqual({ show: true, title: "Shelf" });
    expect(screen.getByText(/Print at 100%/).textContent).toContain('11" x 8.5"');
  });

  it("builds each edit on the one still on its way, and on the kernel's state once it answers", async () => {
    // Enter in Title, then Project at once: the kernel has not answered the first yet.
    const pending = () => new Promise<never>(() => {});
    const p = props({ onSetup: vi.fn(pending), onSetItem: vi.fn(pending), selection: [1] });
    const { rerender } = render(<SheetsPane {...p} />);
    const field = (name: string, value: string) => {
      const f = screen.getByRole("textbox", { name });
      fireEvent.change(f, { target: { value } });
      fireEvent.keyDown(f, { key: "Enter" });
    };
    field("Title", "Bookshelf");
    field("Project", "Shop drawings");
    const calls = () => (p.onSetup as ReturnType<typeof vi.fn>).mock.calls;
    expect(calls().at(-1)![1].title).toEqual({ show: true, title: "Bookshelf", project: "Shop drawings" });
    // Two margins in a row keep each other.
    field("Top margin", "3/4");
    field("Left margin", "1");
    const m = calls().at(-1)![1].margins;
    expect(m[0]).toBeCloseTo(0.75 * IN, 12);
    expect(m[3]).toBeCloseTo(1 * IN, 12);
    expect(calls().at(-1)![1].title.project).toBe("Shop drawings");

    // The kernel's answer arrives (here: it says the title is "Kept"): the next edit builds on it.
    const answered = sheet();
    answered.sheet.title = { show: true, title: "Kept" };
    rerender(<SheetsPane {...p} sheets={[answered]} />);
    field("Revision", "A");
    expect(calls().at(-1)![1].title).toEqual({ show: true, title: "Kept", revision: "A" });

    // A refused edit leaves nothing behind: the next one starts from the state shown.
    (p.onSetup as ReturnType<typeof vi.fn>).mockImplementation(() => Promise.resolve(null));
    field("Drawn by", "nobody");
    await Promise.resolve();
    await Promise.resolve();
    field("Date", "2026-10-12");
    expect(calls().at(-1)![1].title).toEqual({ show: true, title: "Kept", date: "2026-10-12" });

    // The same for an item: a scale, then a caption typed by hand, both kept.
    const item = screen.getByTestId("item-props");
    fireEvent.change(within(item).getByRole("combobox", { name: "Scale" }), { target: { value: "1:8" } });
    const cap = within(item).getByRole("textbox", { name: "Caption" });
    fireEvent.change(cap, { target: { value: "ELEVATION" } });
    fireEvent.keyDown(cap, { key: "Enter" });
    expect(p.onSetItem).toHaveBeenLastCalledWith(1, expect.objectContaining({ scale: { kind: "ratio", paper: 1, model: 8 }, caption: "ELEVATION" }));
  });

  it("edits the selected viewport: scale, source, caption, order", () => {
    const p = props({ selection: [1] });
    render(<SheetsPane {...p} />);
    const item = screen.getByTestId("item-props");
    const scale = within(item).getByRole("combobox", { name: "Scale" });
    expect((scale as HTMLSelectElement).value).toBe("1:12");
    fireEvent.change(scale, { target: { value: "1:8" } });
    // The caption Sawhorse wrote follows the scale.
    expect(p.onSetItem).toHaveBeenLastCalledWith(1, expect.objectContaining({ scale: { kind: "ratio", paper: 1, model: 8 }, caption: "FRONT ELEVATION 1:8" }));
    // An isometric view is never to scale: NTS comes with it.
    fireEvent.change(within(item).getByRole("combobox", { name: "Shows" }), { target: { value: "std:iso" } });
    expect(p.onSetItem).toHaveBeenLastCalledWith(1, expect.objectContaining({ source: { from: "standard", view: "iso" }, scale: { kind: "nts" }, caption: "ISOMETRIC NTS" }));
    fireEvent.click(within(item).getByRole("button", { name: "Fit scale" }));
    expect(p.onFitScale).toHaveBeenCalledWith(1);
    fireEvent.click(within(item).getByRole("button", { name: "Bring to front" }));
    expect(p.onOrder).toHaveBeenCalledWith(1, 2);
    expect((within(item).getByRole("button", { name: "Send to back" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(within(item).getByRole("button", { name: "Delete" }));
    expect(p.onDeleteItems).toHaveBeenCalledWith([1]);
  });

  it("edits a table's columns and a dimension's text", () => {
    const p = props({ selection: [3] });
    const { rerender } = render(<SheetsPane {...p} />);
    fireEvent.click(screen.getByRole("checkbox", { name: "Material" }));
    expect(p.onSetItem).toHaveBeenLastCalledWith(3, expect.objectContaining({ columns: ["part", "qty", "material"] }));
    rerender(<SheetsPane {...p} selection={[2]} />);
    const text = screen.getByRole("textbox", { name: "Text" });
    fireEvent.change(text, { target: { value: "36 (overall)" } });
    fireEvent.keyDown(text, { key: "Enter" });
    expect(p.onSetItem).toHaveBeenLastCalledWith(2, expect.objectContaining({ text: "36 (overall)" }));
  });

  it("keeps both of two column ticks made before the kernel answers (lane 12c)", async () => {
    const pending = () => new Promise<never>(() => {});
    const p = props({ selection: [3], onSetItem: vi.fn(pending) });
    render(<SheetsPane {...p} />);
    fireEvent.click(screen.getByRole("checkbox", { name: "Material" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Qty" }));
    // The second tick builds on the first, still on its way: Material stays, Qty goes.
    expect(p.onSetItem).toHaveBeenLastCalledWith(3, expect.objectContaining({ columns: ["part", "material"] }));
    expect(p.onSetItem).toHaveBeenCalledTimes(2);
  });

  it("gives a viewport its scale back when it comes back from an isometric view (lane 12c)", () => {
    const p = props({ selection: [1] });
    const { rerender } = render(<SheetsPane {...p} />);
    const shows = () => within(screen.getByTestId("item-props")).getByRole("combobox", { name: "Shows" });
    fireEvent.change(shows(), { target: { value: "std:iso" } });
    const sent = (p.onSetItem as ReturnType<typeof vi.fn>).mock.calls.at(-1)![1];
    expect(sent).toMatchObject({ source: { from: "standard", view: "iso" }, scale: { kind: "nts" }, ortho_scale: { kind: "ratio", paper: 1, model: 12 }, caption: "ISOMETRIC NTS" });
    // The kernel's answer: the viewport is isometric now, remembering 1:12.
    const answered = sheet();
    answered.sheet.items[0] = { id: 1, ...sent };
    rerender(<SheetsPane {...p} sheets={[answered]} />);
    fireEvent.change(shows(), { target: { value: "std:top" } });
    expect(p.onSetItem).toHaveBeenLastCalledWith(1, expect.objectContaining({ source: { from: "standard", view: "top" }, scale: { kind: "ratio", paper: 1, model: 12 }, ortho_scale: null, caption: "PLAN 1:12" }));
  });

  it("works out the scale a viewport takes for a view (lane 12c)", () => {
    const eighth = { kind: "ratio" as const, paper: 1, model: 8 };
    const nts = { kind: "nts" as const };
    expect(viewportScaleFor({ scale: eighth }, false)).toEqual({ scale: nts, ortho_scale: eighth });
    expect(viewportScaleFor({ scale: nts, ortho_scale: eighth }, true)).toEqual({ scale: eighth, ortho_scale: null });
    // Isometric to perspective: still remembering.
    expect(viewportScaleFor({ scale: nts, ortho_scale: eighth }, false)).toEqual({ scale: nts, ortho_scale: eighth });
    // Front to Top: the scale stays.
    expect(viewportScaleFor({ scale: eighth }, true)).toEqual({ scale: eighth, ortho_scale: null });
    // NTS chosen by hand on a square view stays NTS.
    expect(viewportScaleFor({ scale: nts }, true)).toEqual({ scale: nts, ortho_scale: null });
    expect(viewportScaleFor({ scale: nts }, false)).toEqual({ scale: nts, ortho_scale: null });
  });

  it("shows the new viewport's source and the dimension kinds with their tools", () => {
    const p = props({ tool: "dimension" });
    const { rerender } = render(<SheetsPane {...p} />);
    fireEvent.click(screen.getByRole("radio", { name: "Angle" }));
    expect(p.onDimKind).toHaveBeenCalledWith("angular");
    rerender(<SheetsPane {...p} tool="viewport" />);
    fireEvent.change(screen.getByRole("combobox", { name: "Shows" }), { target: { value: "scene:Front" } });
    expect(p.onNewSource).toHaveBeenCalledWith({ from: "scene", name: "Front" });
  });

  it("says what to do with no sheet", () => {
    render(<SheetsPane {...props({ sheets: [], active: null })} />);
    expect(screen.getByText(/No sheet yet/)).toBeTruthy();
  });
});

describe("Sheet tabs", () => {
  it("switches mode, shows, adds, renames and moves sheets", () => {
    const onMode = vi.fn();
    const go = vi.fn();
    const add = vi.fn();
    const rename = vi.fn();
    const move = vi.fn();
    const del = vi.fn();
    const rows = [sheet(), { ...sheet(), id: 2, sheet: { ...sheet().sheet, name: "Details" } }];
    render(<SheetTabs lead={<ModeSwitch mode="sheets" onMode={onMode} sheets={2} />} sheets={rows} active={1} onGo={go} onAdd={add} onRename={rename} onMove={move} onDelete={del} />);
    fireEvent.click(screen.getByRole("radio", { name: "Model" }));
    expect(onMode).toHaveBeenCalledWith("model");
    fireEvent.click(screen.getByRole("tab", { name: "Details" }));
    expect(go).toHaveBeenCalledWith(2);
    fireEvent.click(screen.getByRole("button", { name: "Add a sheet" }));
    expect(add).toHaveBeenCalled();
    fireEvent.contextMenu(screen.getByRole("tab", { name: "Details" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Move left" }));
    expect(move).toHaveBeenCalledWith(2, 0);
    fireEvent.doubleClick(screen.getByRole("tab", { name: "Bookshelf" }));
    const input = screen.getByRole("textbox", { name: "New name for sheet Bookshelf" });
    fireEvent.change(input, { target: { value: "Elevations" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(rename).toHaveBeenCalledWith(1, "Elevations");
  });
});
