import { afterEach, describe, expect, it } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { App } from "./App";
import { TOOLS } from "./lib/tools";

afterEach(cleanup);

function pressed(tool: string): boolean {
  return document.querySelector(`.tool[data-tool="${tool}"]`)?.getAttribute("aria-pressed") === "true";
}

describe("App shell", () => {
  it("renders the mock's layout: top bar, rail, viewport, panel, value bar", () => {
    render(<App />);
    expect(document.querySelector(".app > header.top")).not.toBeNull();
    expect(document.querySelector(".app > aside.rail")).not.toBeNull();
    expect(document.querySelector(".app > main.vp")).not.toBeNull();
    expect(document.querySelector(".app > aside.side")).not.toBeNull();
    expect(document.querySelector(".app > footer.vcb")).not.toBeNull();
    expect(screen.getByText("Credits")).toBeTruthy();
    for (const v of ["Iso", "Top", "Front", "Right", "Persp"]) expect(screen.getByRole("button", { name: v })).toBeTruthy();
  });

  it("shows every tool with its 2017-default key cap", () => {
    render(<App />);
    const caps = [...document.querySelectorAll(".tool kbd")].map((k) => k.textContent);
    expect(caps).toEqual(["␣", "E", "L", "R", "C", "A", "P", "M", "Q", "S", "F", "W", "T", "D", "B"]);
    expect(TOOLS).toHaveLength(15);
    expect(pressed("select")).toBe(true);
  });

  it("switches tools from the keyboard", () => {
    render(<App />);
    const keys: [string, string][] = [
      ["e", "erase"], ["l", "line"], ["r", "rect"], ["c", "circle"], ["a", "arc"], ["p", "pushpull"], ["m", "move"],
      ["q", "rotate"], ["s", "scale"], ["f", "offset"], ["w", "follow"], ["t", "tape"], ["d", "dim"], ["b", "paint"], [" ", "select"],
    ];
    for (const [key, tool] of keys) {
      act(() => {
        fireEvent.keyDown(window, { key });
      });
      expect(pressed(tool)).toBe(true);
    }
  });

  it("does not steal keys typed into the prompt box", () => {
    render(<App />);
    fireEvent.click(screen.getByRole("tab", { name: "Render" }));
    const prompt = screen.getByLabelText("Prompt");
    fireEvent.keyDown(prompt, { key: "p" });
    expect(pressed("select")).toBe(true);
  });

  it("has the four right-panel tabs: a live Outliner and Tags, a cut list that says it is not built yet", () => {
    render(<App />);
    const tabs = screen.getAllByRole("tab").map((t) => t.textContent);
    expect(tabs).toEqual(["Outliner", "Tags", "Cut list", "Render"]);
    // No example content: an empty model has an empty Outliner, and Tags has its default.
    expect(screen.queryByText("Side L")).toBeNull();
    expect(screen.getByText(/Nothing yet/)).toBeTruthy();
    fireEvent.click(screen.getByRole("tab", { name: "Tags" }));
    expect(screen.getByText("Untagged")).toBeTruthy();
    expect(screen.getByRole("button", { name: /New tag/ })).toBeTruthy();
    fireEvent.click(screen.getByRole("tab", { name: "Cut list" }));
    expect(document.getElementById("pane-cutlist")?.hidden).toBe(false);
    expect(document.getElementById("pane-outliner")?.hidden).toBe(true);
    expect(screen.getByTestId("cutlist-placeholder").textContent).toMatch(/not built yet/);
    expect(screen.queryByText("board feet, solid oak")).toBeNull();
    fireEvent.click(screen.getByRole("tab", { name: "Render" }));
    fireEvent.click(screen.getByRole("button", { name: /Kitchen/ }));
    expect((screen.getByLabelText("Prompt") as HTMLTextAreaElement).value).toMatch(/warm kitchen/);
  });

  it("types straight into the value bar mid-gesture and cycles units", () => {
    render(<App />);
    act(() => {
      fireEvent.keyDown(window, { key: "p" });
    });
    expect(screen.getByText("Distance")).toBeTruthy();
    const input = screen.getByLabelText("Value") as HTMLInputElement;
    expect(input.value).toBe("12");
    act(() => {
      fireEvent.keyDown(window, { key: "3" });
    });
    expect(input.value).toBe("3");
    expect(document.activeElement).toBe(input);
    const unitBtn = document.querySelector(".vcb button.units") as HTMLButtonElement;
    expect(unitBtn.textContent).toBe("in");
    expect(screen.getByText("Units: fractional in")).toBeTruthy();
    fireEvent.click(unitBtn); // decimal in
    fireEvent.click(unitBtn); // ft-in
    fireEvent.click(unitBtn); // mm
    expect(unitBtn.textContent).toBe("mm");
    expect(screen.getByText("Units: mm")).toBeTruthy();
  });

  it("reports unreadable values without touching the model", () => {
    render(<App />);
    act(() => {
      fireEvent.keyDown(window, { key: "p" });
    });
    const input = screen.getByLabelText("Value") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "banana" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(document.getElementById("hint")?.className).toContain("err");
    expect(document.getElementById("hint")?.textContent).toMatch(/banana/);
  });
});
