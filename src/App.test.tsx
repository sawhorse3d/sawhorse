import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { App, describeImport, plural } from "./App";
import { RENDER_URL_KEY } from "./credits/gate";
import { TOOLS } from "./lib/tools";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  localStorage.removeItem(RENDER_URL_KEY);
});

/** Lane 15: a render service set in Settings that answers its health check (and nothing else). */
function renderServiceUp() {
  localStorage.setItem(RENDER_URL_KEY, "https://render.test");
  vi.stubGlobal("fetch", (url: string) => (String(url) === "https://render.test/healthz" ? Promise.resolve(new Response(JSON.stringify({ ok: true, service: "sawhorse-render", version: "0.1.0" }), { status: 200 })) : Promise.reject(new TypeError("offline"))));
}

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
    // Lane 15: no render service answering here, so no Credits chip.
    expect(screen.queryByText("Credits")).toBeNull();
    for (const v of ["Iso", "Top", "Front", "Right", "Persp"]) expect(screen.getByRole("button", { name: v })).toBeTruthy();
  });

  it("shows every tool with its 2017-default key cap", () => {
    render(<App />);
    const caps = [...document.querySelectorAll(".tool kbd")].map((k) => k.textContent);
    expect(caps).toEqual(["␣", "E", "L", "R", "C", "A", "P", "M", "Q", "S", "F", "W", "T", "D", "B", "O", "H"]);
    // Lane 12's Section Plane has no default key (SketchUp 2017 had none either); lane 30's
    // Orbit and Pan have SketchUp 2017's O and H.
    expect(TOOLS).toHaveLength(18);
    expect(TOOLS.find((t) => t.id === "section")?.key).toBe("");
    expect(document.querySelector('.tool[data-tool="section"] kbd')).toBeNull();
    expect(pressed("select")).toBe(true);
  });

  it("switches tools from the keyboard", () => {
    render(<App />);
    const keys: [string, string][] = [
      ["e", "erase"], ["l", "line"], ["r", "rect"], ["c", "circle"], ["a", "arc"], ["p", "pushpull"], ["m", "move"],
      ["q", "rotate"], ["s", "scale"], ["f", "offset"], ["w", "follow"], ["t", "tape"], ["d", "dim"], ["b", "paint"], ["o", "orbit"], ["h", "pan"], [" ", "select"],
    ];
    for (const [key, tool] of keys) {
      act(() => {
        fireEvent.keyDown(window, { key });
      });
      expect(pressed(tool)).toBe(true);
    }
  });

  it("O and H pick Orbit and Pan (lane 30, #36); Esc goes back to the tool before them", async () => {
    render(<App />);
    const key = async (k: string) => {
      await act(async () => {
        fireEvent.keyDown(window, { key: k });
      });
    };
    const hint = () => document.getElementById("hint")?.textContent ?? "";
    await key("l");
    expect(pressed("line")).toBe(true);
    await key("o");
    expect(pressed("orbit")).toBe(true);
    expect(hint()).toBe("Orbit. Drag to orbit around the point under the cursor. Shift+drag pans. Esc goes back to the tool before.");
    expect(document.querySelector('.tool[data-tool="orbit"] kbd')?.textContent).toBe("O");
    expect((document.getElementById("cv") as HTMLCanvasElement).style.cursor).toBe("grab");
    // Orbit, then Pan: Esc still goes back to Line, the tool before both.
    await key("h");
    expect(pressed("pan")).toBe(true);
    expect(hint()).toBe("Pan. Drag to pan: the point you grab stays under the cursor. Esc goes back to the tool before.");
    await key("Escape");
    expect(pressed("line")).toBe(true);
    // From Select (the rail's button this time), Esc goes back to Select.
    await key(" ");
    fireEvent.click(document.querySelector('.tool[data-tool="orbit"]')!);
    expect(pressed("orbit")).toBe(true);
    await key("Escape");
    expect(pressed("select")).toBe(true);
    // Esc in Select is Select's own again (nothing to go back to).
    await key("Escape");
    expect(pressed("select")).toBe(true);
  });

  it("the View menu has Orbit (O) and Pan (H), checked while active (lane 30)", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "View" }));
    const orbit = screen.getByRole("menuitemcheckbox", { name: /Orbit\s*O$/ });
    expect(orbit.getAttribute("aria-checked")).toBe("false");
    expect(screen.getByRole("menuitemcheckbox", { name: /Pan\s*H$/ })).toBeTruthy();
    await act(async () => {
      fireEvent.click(orbit);
    });
    expect(pressed("orbit")).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "View" }));
    expect(screen.getByRole("menuitemcheckbox", { name: /Orbit\s*O$/ }).getAttribute("aria-checked")).toBe("true");
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitemcheckbox", { name: /Pan\s*H$/ }));
    });
    expect(pressed("pan")).toBe(true);
  });

  it("an Alt tap is a press and release with nothing between: Alt held for an Alt+drag is not one (lane 30)", async () => {
    render(<App />);
    const hint = () => document.getElementById("hint")?.textContent ?? "";
    await act(async () => {
      fireEvent.keyDown(window, { key: "m" });
    });
    expect(pressed("move")).toBe(true);
    // A tap switches Move's autofold, on the release.
    await act(async () => {
      fireEvent.keyDown(window, { key: "Alt", altKey: true });
    });
    expect(hint()).not.toMatch(/Autofold/);
    await act(async () => {
      fireEvent.keyUp(window, { key: "Alt" });
    });
    expect(hint()).toMatch(/^Move\. Autofold on/);
    // Alt held while the button goes down (an Alt+drag orbit): no switch.
    await act(async () => {
      fireEvent.keyDown(window, { key: "Escape" });
      fireEvent.keyDown(window, { key: "Alt", altKey: true });
      fireEvent.pointerDown(document.getElementById("cv")!, { button: 0, buttons: 1, altKey: true });
      fireEvent.pointerUp(document.getElementById("cv")!, { button: 0, altKey: true });
      fireEvent.keyUp(window, { key: "Alt" });
    });
    expect(hint()).not.toMatch(/Autofold/);
    // Nor with another key between (Alt+Shift+K, a plugin's shortcut).
    await act(async () => {
      fireEvent.keyDown(window, { key: "Alt", altKey: true });
      fireEvent.keyDown(window, { key: "K", altKey: true, shiftKey: true });
      fireEvent.keyUp(window, { key: "Alt" });
    });
    expect(hint()).not.toMatch(/Autofold/);
    // And the next tap switches it back off.
    await act(async () => {
      fireEvent.keyDown(window, { key: "Alt", altKey: true });
      fireEvent.keyUp(window, { key: "Alt" });
    });
    expect(hint()).toMatch(/^Move\. Autofold off/);
  });

  it("macOS: a Control tap is its release with nothing between, so a Control+drag pan is not one", async () => {
    const platform = vi.spyOn(navigator, "platform", "get").mockReturnValue("MacIntel");
    try {
      render(<App />);
      const hint = () => document.getElementById("hint")?.textContent ?? "";
      await act(async () => {
        fireEvent.keyDown(window, { key: "m" });
      });
      expect(pressed("move")).toBe(true);
      // A tap switches Move's copying, on the release.
      await act(async () => {
        fireEvent.keyDown(window, { key: "Control", ctrlKey: true });
      });
      expect(hint()).not.toMatch(/Copying/);
      await act(async () => {
        fireEvent.keyUp(window, { key: "Control" });
      });
      expect(hint()).toMatch(/Copying/);
      // Control held while the button goes down (a Control+drag pan): no switch.
      await act(async () => {
        fireEvent.keyDown(window, { key: "Control", ctrlKey: true });
        fireEvent.pointerDown(document.getElementById("cv")!, { button: 0, buttons: 1, ctrlKey: true });
        fireEvent.pointerUp(document.getElementById("cv")!, { button: 0, ctrlKey: true });
        fireEvent.keyUp(window, { key: "Control" });
      });
      expect(hint()).toMatch(/Copying/);
      // Nor with another key between (Control+Z).
      await act(async () => {
        fireEvent.keyDown(window, { key: "Control", ctrlKey: true });
        fireEvent.keyDown(window, { key: "z", ctrlKey: true });
        fireEvent.keyUp(window, { key: "Control" });
      });
      expect(hint()).not.toMatch(/Moving \(not copying\)/);
      // And the next tap switches it back.
      await act(async () => {
        fireEvent.keyDown(window, { key: "Control", ctrlKey: true });
        fireEvent.keyUp(window, { key: "Control" });
      });
      expect(hint()).toMatch(/Moving \(not copying\)/);
    } finally {
      platform.mockRestore();
    }
  });

  it("does not steal keys typed into the prompt box", async () => {
    renderServiceUp();
    render(<App />);
    fireEvent.click(await screen.findByRole("tab", { name: "Render" }));
    const prompt = screen.getByLabelText("Prompt");
    fireEvent.keyDown(prompt, { key: "p" });
    expect(pressed("select")).toBe(true);
  });

  it("has the right-panel tabs: a live Outliner, Tags, Cut list, Materials, Styles, Render (with a render service), and the Plugins glyph tab", async () => {
    renderServiceUp();
    render(<App />);
    await screen.findByRole("tab", { name: "Render" });
    const tabs = screen.getAllByRole("tab").map((t) => t.getAttribute("aria-label") ?? t.textContent);
    expect(tabs).toEqual(["Outliner", "Tags", "Cut list", "Materials", "Styles", "Render", "Plugins"]);
    expect(screen.getByText("Credits")).toBeTruthy();
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

  it("without a render service: no Credits chip, no Render pane, and Settings says why in one line (lane 15)", async () => {
    vi.stubGlobal("fetch", () => Promise.reject(new TypeError("offline")));
    render(<App />);
    expect(screen.queryByText("Credits")).toBeNull();
    expect(screen.getAllByRole("tab").map((t) => t.getAttribute("aria-label") ?? t.textContent)).toEqual(["Outliner", "Tags", "Cut list", "Materials", "Styles", "Plugins"]);
    // Edit > Settings…
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Settings/ }));
    const line = await screen.findByTestId("render-gate");
    await vi.waitFor(() => expect(line.getAttribute("data-status")).toMatch(/off|unreachable/));
    expect(line.textContent).toMatch(/^Renders are off/);
    // A service that answers turns renders on.
    vi.stubGlobal("fetch", (url: string) => (String(url) === "https://render.test/healthz" ? Promise.resolve(new Response(JSON.stringify({ ok: true, service: "sawhorse-render", version: "0.1.0" }), { status: 200 })) : Promise.reject(new TypeError("offline"))));
    fireEvent.change(screen.getByLabelText("Render service address"), { target: { value: "https://render.test/" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await vi.waitFor(() => expect(screen.getByTestId("render-gate").getAttribute("data-status")).toBe("ready"));
    expect(screen.getByTestId("render-gate").textContent).toBe("Renders are on: the render service at https://render.test answered. The Credits chip and the Render pane are shown.");
    expect(localStorage.getItem(RENDER_URL_KEY)).toBe("https://render.test");
    expect(await screen.findByRole("tab", { name: "Render" })).toBeTruthy();
    expect(screen.getByText("Credits")).toBeTruthy();
    // Cleared: off again.
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    await vi.waitFor(() => expect(screen.queryByRole("tab", { name: "Render" })).toBeNull());
  });

  it("types straight into the value bar mid-gesture, and picks units from the chip (lane 24)", () => {
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
    expect(unitBtn.getAttribute("aria-label")).toBe("Units: fractional inches. Click to change.");
    expect(screen.getByText("Units: fractional in")).toBeTruthy();
    // The chip opens the picker (it used to cycle); one pick is one change.
    fireEvent.click(unitBtn);
    fireEvent.click(screen.getByRole("menuitemradio", { name: /millimetres/i }));
    expect(screen.queryByRole("menu")).toBeNull();
    expect(unitBtn.textContent).toBe("mm");
    expect(screen.getByText("Units: mm")).toBeTruthy();
    expect(localStorage.getItem("sawhorse.units")).toBe("mm");
    // The box's default follows: 12 in is 304.8 mm.
    expect(input.value).toBe("304.8");
    // Edit > Units lists the same six, the current one checked.
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Units" }));
    const sub = screen.getByRole("menu", { name: "Units" });
    const boxes = [...sub.querySelectorAll('[role="menuitemcheckbox"]')];
    expect(boxes.map((b) => b.textContent?.trim())).toEqual(["✓ Millimetres", "Centimetres", "Metres", "Decimal inches", "Feet and inches", "Fractional inches"]);
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "Centimetres" }));
    expect(unitBtn.textContent).toBe("cm");
    expect(screen.getByText("Units: cm")).toBeTruthy();
    // Edit > Units > Choose… opens the chip's picker.
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Units" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Choose…" }));
    expect(screen.getByRole("menu", { name: "Units" }).getAttribute("data-testid")).toBe("unit-picker");
    localStorage.removeItem("sawhorse.units");
  });

  it("no units hint outside the browser demo (lane 24)", () => {
    render(<App />);
    expect(screen.queryByRole("note", { name: "Where the units are" })).toBeNull();
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

describe("keyboard after a pane control (lane 15)", () => {
  it("a select picked with the mouse gives the keyboard back: r picks Rectangle", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("tab", { name: "Cut list" }));
    const sel = screen.getByLabelText("Round sizes to") as HTMLSelectElement;
    sel.focus();
    fireEvent.pointerDown(sel);
    fireEvent.change(sel, { target: { value: "half_mm" } });
    await new Promise((r) => setTimeout(r, 5));
    expect(document.activeElement).not.toBe(sel);
    act(() => {
      fireEvent.keyDown(document.activeElement ?? window, { key: "r" });
    });
    expect(pressed("rect")).toBe(true);
  });

  it("a select worked from the keyboard keeps focus (its arrows still move it)", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("tab", { name: "Cut list" }));
    const sel = screen.getByLabelText("Round sizes to") as HTMLSelectElement;
    sel.focus();
    fireEvent.keyDown(sel, { key: "ArrowDown" });
    fireEvent.change(sel, { target: { value: "half_mm" } });
    await new Promise((r) => setTimeout(r, 5));
    expect(document.activeElement).toBe(sel);
  });
});
