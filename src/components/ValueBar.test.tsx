import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ValueBar } from "./ValueBar";
import { UNITS_HINT_KEY, UNITS_HINT_TEXT } from "./UnitsHint";
import { unitChoiceText } from "./UnitPicker";
import { TOOLS } from "../lib/tools";
import { UNIT_CHOICES, type UnitSystem } from "../lib/units";

// Lane 24 (issue #13: "Trying in the web app and it only gives me inches"): the unit chip
// opens a picker, the status line names the units on an empty document, and the browser
// demo shows a one-sentence hint once.

const push = TOOLS.find((t) => t.id === "pushpull")!;
const scale = TOOLS.find((t) => t.id === "scale")!;

afterEach(cleanup);
beforeEach(() => localStorage.clear());

function bar(over: Partial<Parameters<typeof ValueBar>[0]> = {}) {
  const onUnits = vi.fn();
  const props = { tool: push, value: "12", onChange: () => {}, onCommit: () => {}, onCancel: () => {}, units: "frac-in" as UnitSystem, onUnits, message: null, ...over };
  const r = render(<ValueBar {...props} />);
  return { ...r, onUnits, rerender: (more: Partial<typeof props>) => r.rerender(<ValueBar {...props} {...more} />) };
}

describe("the unit chip and its picker", () => {
  it("the chip says what it is and opens the picker; a pick reports the unit and closes it", () => {
    const { onUnits } = bar();
    const chip = screen.getByRole("button", { name: "Units: fractional inches. Click to change." });
    expect(chip.textContent).toBe("in");
    expect(chip.getAttribute("aria-haspopup")).toBe("menu");
    expect(chip.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByRole("menu")).toBeNull();
    fireEvent.click(chip);
    const menu = screen.getByRole("menu", { name: "Units" });
    expect(chip.getAttribute("aria-expanded")).toBe("true");
    // Every unit system once, metric first, the current one checked and focused.
    const items = screen.getAllByRole("menuitemradio");
    expect(items.map((b) => b.getAttribute("data-unit"))).toEqual(["mm", "cm", "m", "in", "ft-in", "frac-in"]);
    expect(items.map((b) => b.getAttribute("aria-checked"))).toEqual(["false", "false", "false", "false", "false", "true"]);
    expect(document.activeElement).toBe(items[5]);
    expect(menu.textContent).toContain("Millimetres");
    expect(menu.textContent).toContain("Feet and inches");
    fireEvent.click(screen.getByRole("menuitemradio", { name: /millimetres/i }));
    expect(onUnits).toHaveBeenCalledWith("mm");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(chip.getAttribute("aria-expanded")).toBe("false");
  });

  it("each choice shows 0.3 m written in that unit", () => {
    bar();
    fireEvent.click(screen.getByRole("button", { name: /^Units:/ }));
    const samples = screen.getAllByRole("menuitemradio").map((b) => b.querySelector("kbd")?.textContent);
    expect(samples).toEqual(["300 mm", "30 cm", "0.3 m", "11.811 in", '11 13/16"', "11 13/16 in"]);
    for (const id of UNIT_CHOICES) expect(unitChoiceText(id).sample).toBe(samples[UNIT_CHOICES.indexOf(id)]);
  });

  it("Esc, a click outside, or the chip again closes it without a pick; the arrows move the focus", () => {
    const { onUnits } = bar();
    const chip = screen.getByRole("button", { name: /^Units:/ });
    fireEvent.click(chip);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    fireEvent.click(chip);
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("menu")).toBeNull();
    fireEvent.click(chip);
    expect(screen.getByRole("menu")).toBeTruthy();
    fireEvent.click(chip);
    expect(screen.queryByRole("menu")).toBeNull();
    fireEvent.click(chip);
    const items = screen.getAllByRole("menuitemradio");
    fireEvent.keyDown(items[5], { key: "ArrowDown" });
    expect(document.activeElement).toBe(items[0]);
    fireEvent.keyDown(items[0], { key: "ArrowUp" });
    expect(document.activeElement).toBe(items[5]);
    expect(onUnits).not.toHaveBeenCalled();
  });

  it("the chip reads the current unit; a factor has no chip but Edit > Units can still open the picker", () => {
    const { rerender } = bar({ units: "mm" });
    expect(screen.getByRole("button", { name: "Units: millimetres. Click to change." }).textContent).toBe("mm");
    rerender({ tool: scale });
    expect(screen.queryByRole("button", { name: /^Units:/ })).toBeNull();
    expect(screen.getByTestId("vcb-unit").textContent).toBe("");
    rerender({ tool: scale, openPicker: 1 });
    expect(screen.getByRole("menu", { name: "Units" })).toBeTruthy();
  });
});

describe("the status line on an empty document", () => {
  it("names the units and is the click that changes them", () => {
    const { onUnits } = bar({ empty: true });
    const line = document.getElementById("hint")!;
    expect(line.textContent).toBe("Push. Click a face and move. Type 3/4 for 3/4 in. Double-click repeats the last distance.Units: fractional inches. Click to change.");
    const link = screen.getByTestId("hint-units");
    expect(link.tagName).toBe("BUTTON");
    expect(link.textContent).toBe("Units: fractional inches. Click to change.");
    fireEvent.click(link);
    fireEvent.click(screen.getByRole("menuitemradio", { name: /centimetres/i }));
    expect(onUnits).toHaveBeenCalledWith("cm");
  });

  it("gives way to a message, and goes once something is drawn", () => {
    const { rerender } = bar({ empty: true, message: { text: "Extruded 300 mm.", error: false } });
    expect(screen.queryByTestId("hint-units")).toBeNull();
    rerender({ empty: false, message: null });
    expect(screen.queryByTestId("hint-units")).toBeNull();
    rerender({ empty: true, message: null });
    expect(screen.getByTestId("hint-units")).toBeTruthy();
  });
});

describe("the browser demo's first-run hint", () => {
  it("is one sentence that says where units live, shown only when asked for", () => {
    bar();
    expect(screen.queryByRole("note")).toBeNull();
    cleanup();
    bar({ firstRunHint: true });
    const note = screen.getByRole("note", { name: "Where the units are" });
    expect(note.textContent).toContain(UNITS_HINT_TEXT);
    expect(UNITS_HINT_TEXT.split(/[.!?]\s/).length).toBe(1);
    expect(UNITS_HINT_TEXT).toMatch(/unit chip/);
    expect(UNITS_HINT_TEXT).toMatch(/Edit > Units/);
    // Its button is not called Dismiss: the demo banner's is, and the site smoke finds that one by name.
    expect(screen.getByRole("button", { name: "Got it" })).toBeTruthy();
  });

  it("Got it dismisses it for good on this browser", () => {
    bar({ firstRunHint: true });
    fireEvent.click(screen.getByRole("button", { name: "Got it" }));
    expect(screen.queryByRole("note")).toBeNull();
    expect(localStorage.getItem(UNITS_HINT_KEY)).toBe("1");
    cleanup();
    bar({ firstRunHint: true });
    expect(screen.queryByRole("note")).toBeNull();
  });

  it("opening the picker counts as finding the units", () => {
    bar({ firstRunHint: true });
    fireEvent.click(screen.getByRole("button", { name: /^Units:/ }));
    expect(screen.queryByRole("note")).toBeNull();
    expect(localStorage.getItem(UNITS_HINT_KEY)).toBe("1");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("note")).toBeNull();
  });

  it("still dismisses when storage throws (private windows)", () => {
    const set = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("blocked", "SecurityError");
    });
    const get = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new DOMException("blocked", "SecurityError");
    });
    bar({ firstRunHint: true });
    fireEvent.click(screen.getByRole("button", { name: "Got it" }));
    expect(screen.queryByRole("note")).toBeNull();
    set.mockRestore();
    get.mockRestore();
  });
});
