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

describe("a message's buttons answer once (#145)", () => {
  // With more than one set of unsaved work, answering the offer puts the next offer in the
  // same place at once: a second click or key press meant for the first must not answer it.
  const offerWith = (run: () => void, text = "Restore unsaved work? 1 solid, 3 steps.") => ({ text, error: false, offer: true, actions: [{ label: "Restore", run: () => {} }, { label: "Discard", run }] });

  it("the second click of a double-click runs nothing; a single click and a key press (no clicks) run the action", () => {
    const run = vi.fn();
    bar({ message: offerWith(run) });
    const discard = screen.getByRole("button", { name: "Discard" });
    fireEvent.click(discard, { detail: 1 });
    fireEvent.click(discard, { detail: 2 });
    fireEvent.click(discard, { detail: 3 });
    expect(run).toHaveBeenCalledTimes(1);
    fireEvent.click(discard, { detail: 0 });
    expect(run).toHaveBeenCalledTimes(2);
  });

  it("the button that answered gives up the focus, so the next offer's Discard in its place does not have it", () => {
    const first = vi.fn();
    const next = vi.fn();
    const { rerender } = bar({ message: offerWith(first) });
    const discard = screen.getByRole("button", { name: "Discard" });
    discard.focus();
    expect(document.activeElement).toBe(discard);
    fireEvent.click(discard, { detail: 0 });
    expect(first).toHaveBeenCalledTimes(1);
    rerender({ message: offerWith(next, "Unsaved work discarded. Restore unsaved work? 1 solid, 5 steps.") });
    const again = screen.getByRole("button", { name: "Discard" });
    expect(document.activeElement).not.toBe(again);
    // Given up to the page itself, not to another control Enter would press or commit.
    expect(document.activeElement).toBe(document.body);
    expect(next).not.toHaveBeenCalled();
  });

  it("a second click that answers nothing gives up the focus too: its press can give it to the next offer's button", () => {
    const first = vi.fn();
    const next = vi.fn();
    const { rerender } = bar({ message: offerWith(first) });
    fireEvent.click(screen.getByRole("button", { name: "Discard" }), { detail: 1 });
    rerender({ message: offerWith(next, "Unsaved work discarded. Restore unsaved work? 1 solid, 5 steps.") });
    // The second press lands on the next offer's Restore and focuses it, as a mouse press does.
    const restore = screen.getByRole("button", { name: "Restore" });
    restore.focus();
    fireEvent.click(restore, { detail: 2 });
    expect(document.activeElement).toBe(document.body);
    expect(first).toHaveBeenCalledTimes(1);
    expect(next).not.toHaveBeenCalled();
  });

  it("so does every button of every message, not only the offer's Discard", () => {
    const messages = [
      { text: "Restore unsaved work? 1 solid, 3 steps.", error: false, offer: true, labels: ["Restore", "Discard"] },
      { text: "Could not restore the unsaved work: it cannot be read.", error: true, offer: true, labels: ["Discard"] },
      { text: "Restored your unsaved work. shelf.shz is not open yet.", error: false, startup: true, labels: ["Open shelf.shz"] },
      { text: "Importing cabinet.skp…", error: false, labels: ["Cancel"] },
    ];
    for (const { labels, ...m } of messages) {
      const runs = labels.map(() => vi.fn());
      const { unmount } = bar({ message: { ...m, actions: labels.map((label, i) => ({ label, run: runs[i] })) } });
      labels.forEach((label, i) => {
        const b = screen.getByRole("button", { name: label });
        b.focus();
        fireEvent.click(b, { detail: 1 });
        fireEvent.click(b, { detail: 2 });
        expect(runs[i]).toHaveBeenCalledTimes(1);
        expect(document.activeElement).toBe(document.body);
      });
      unmount();
    }
  });
});

// Lane 64 (#94): while unsaved work is set aside and its offer is no longer the status line's
// message (an edit replaced it), the line says so, and a click shows the offer again.
describe("the reminder of unsaved work set aside", () => {
  it("says how much is set aside after the message, and its click shows the offer", () => {
    const onShow = vi.fn();
    const { rerender } = bar({ message: { text: "Extruded 12 in.", error: false }, setAside: { count: 2, onShow } });
    const hint = document.getElementById("hint")!;
    const note = screen.getByTestId("hint-offers");
    expect(note.textContent).toBe("Unsaved work set aside (2). Click to restore or discard.");
    expect(hint.textContent).toContain("Extruded 12 in.");
    fireEvent.click(note);
    expect(onShow).toHaveBeenCalledTimes(1);
    rerender({ setAside: { count: 1, onShow } });
    expect(screen.getByTestId("hint-offers").textContent).toBe("Unsaved work set aside. Click to restore or discard.");
    // The tool's own hint (no message) keeps it too.
    rerender({ message: null, setAside: { count: 1, onShow } });
    expect(screen.getByTestId("hint-offers")).toBeTruthy();
  });

  it("is not there with nothing set aside", () => {
    const { rerender } = bar({ message: { text: "Extruded 12 in.", error: false } });
    expect(screen.queryByTestId("hint-offers")).toBeNull();
    rerender({ setAside: null });
    expect(screen.queryByTestId("hint-offers")).toBeNull();
    rerender({ setAside: { count: 0, onShow: () => {} } });
    expect(screen.queryByTestId("hint-offers")).toBeNull();
  });
});
