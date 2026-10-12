import { afterEach, describe, expect, it } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { App, describeImport, plural } from "./App";
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

  it("has the right-panel tabs: a live Outliner, Tags, Cut list, Materials, Styles, Render, and the Plugins glyph tab", () => {
    render(<App />);
    const tabs = screen.getAllByRole("tab").map((t) => t.getAttribute("aria-label") ?? t.textContent);
    expect(tabs).toEqual(["Outliner", "Tags", "Cut list", "Materials", "Styles", "Render", "Plugins"]);
    // No example content: an empty model has an empty Outliner, and Tags has its default.
    expect(screen.queryByText("Side L")).toBeNull();
    expect(screen.getByText(/Nothing yet/)).toBeTruthy();
    fireEvent.click(screen.getByRole("tab", { name: "Tags" }));
    expect(screen.getByText("Untagged")).toBeTruthy();
    expect(screen.getByRole("button", { name: /New tag/ })).toBeTruthy();
    fireEvent.click(screen.getByRole("tab", { name: "Cut list" }));
    expect(document.getElementById("pane-cutlist")?.hidden).toBe(false);
    expect(document.getElementById("pane-outliner")?.hidden).toBe(true);
    // No example rows: without a kernel (jsdom) the pane says why it is empty, and its
    // exports wait for a list.
    expect(screen.getByTestId("cutlist")).toBeTruthy();
    expect(screen.getByText(/worked out by the kernel/)).toBeTruthy();
    expect(screen.queryByText("board feet, solid oak")).toBeNull();
    expect((screen.getByRole("button", { name: "CSV" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: /Nesting/ }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByTestId("cutlist-note").textContent).toContain("rough area estimate (Nesting lays the parts out");
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

  it("has the Materials and Styles panes (lane 10)", () => {
    render(<App />);
    fireEvent.click(screen.getByRole("tab", { name: "Materials" }));
    expect(document.getElementById("pane-materials")?.hidden).toBe(false);
    expect(screen.getByTestId("materials")).toBeTruthy();
    expect(screen.getByText("No material")).toBeTruthy();
    fireEvent.click(screen.getByRole("tab", { name: "Styles" }));
    expect(screen.getAllByRole("button", { name: /Sawhorse|Shop drawing|Shaded|X-ray|Clay|Outdoors/ })).toHaveLength(6);
    expect(screen.getByLabelText("Time")).toBeTruthy();
  });

  it("Paint with no material brings the Materials pane forward", () => {
    render(<App />);
    act(() => {
      fireEvent.keyDown(window, { key: "b" });
    });
    expect(pressed("paint")).toBe(true);
    expect(document.getElementById("pane-materials")?.hidden).toBe(false);
  });
});

describe("plural and the import line", () => {
  it("adds s, and es after s, x, sh and ch (not 'meshs' or 'sketchs')", () => {
    expect(plural(1, "mesh")).toBe("1 mesh");
    expect(plural(2, "mesh")).toBe("2 meshes");
    expect(plural(37, "mesh")).toBe("37 meshes");
    expect(plural(0, "mesh")).toBe("0 meshes");
    expect(plural(3, "sketch")).toBe("3 sketches");
    expect(plural(2, "box")).toBe("2 boxes");
    expect(plural(2, "class")).toBe("2 classes");
    expect(plural(1, "solid")).toBe("1 solid");
    expect(plural(2, "solid")).toBe("2 solids");
    expect(plural(2, "component instance")).toBe("2 component instances");
    expect(plural(2, "imported mesh")).toBe("2 imported meshes");
  });

  it("describeImport says '37 meshes that are not solid'", () => {
    const outcome = { kind: "imported", objects: [1, 2], meshes: Array.from({ length: 37 }, (_, i) => i + 10), groups: [], sketches: [], notes: [] } as unknown as Parameters<typeof describeImport>[1];
    expect(describeImport("Untitled.skp", outcome)).toBe("Imported Untitled.skp: 2 solids, 37 meshes that are not solid.");
    const one = { ...outcome, meshes: [10] } as typeof outcome;
    expect(describeImport("a.skp", one)).toBe("Imported a.skp: 2 solids, 1 mesh that is not solid.");
    const restored = { kind: "restored", objects: 3, meshes: 4 } as unknown as Parameters<typeof describeImport>[1];
    expect(describeImport("a.shz", restored)).toBe("Opened a.shz: 3 solids, 4 meshes (not solid).");
  });
});
