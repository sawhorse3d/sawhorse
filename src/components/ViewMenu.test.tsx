// #107: the 3D view's right-click menu offers the menu bar's own commands, by what was
// right-clicked, and closes on Escape, a press outside, the wheel or a choice.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { itemsOf } from "../App";
import type { Entity } from "../kernel/types";
import type { MenuItem } from "./TopBar";
import { menuTarget, ViewMenu, viewMenuItems, type ViewMenuTarget } from "./ViewMenu";

afterEach(cleanup);

const cmd = (label: string, extra: Partial<{ keys: string; disabled: boolean }> = {}): MenuItem => ({ label, onSelect: vi.fn(), ...extra });

/** The menu bar as App builds it, cut down: an Edit menu with every command the view menu
 * takes, some the view menu never offers, and a View menu with Zoom Extents. */
function bar(section: boolean) {
  return {
    Edit: [
      cmd("Undo Push/Pull", { keys: "Ctrl+Z" }),
      cmd("Redo", { keys: "Ctrl+Y", disabled: true }),
      "-",
      cmd("Select All", { keys: "Ctrl+A" }),
      ...(section ? ["-" as const, cmd("Deactivate Cut"), cmd("Reverse Section Plane"), cmd("Create Group from Slice"), "-" as const] : []),
      cmd("Select None", { keys: "Esc" }),
      cmd("Delete", { keys: "Delete" }),
      cmd("Delete Guides"),
      "-",
      cmd("Make Group", { keys: "Ctrl+G" }),
      cmd("Make Component…"),
      cmd("Make Unique", { disabled: true }),
      cmd("Explode", { keys: "Ctrl+Shift+G", disabled: true }),
      cmd("Explode Curve", { disabled: true }),
      cmd("Clear Paint"),
      cmd("Hide"),
      cmd("Lock"),
      cmd("Edit Group / Component", { disabled: true }),
      cmd("Close Group / Component", { keys: "Esc" }),
      cmd("Delete Imported Meshes"),
      { label: "Units", items: [cmd("Millimetres")] },
    ] as MenuItem[],
    View: [cmd("Zoom Extents", { keys: "Shift+Z" }), cmd("Orbit")] as MenuItem[],
  };
}

const labels = (items: MenuItem[]) => items.map((it) => (it === "-" ? "-" : it.label));

describe("which commands a right-click offers", () => {
  it("on the selection: the edits of the Edit menu, the same objects, so the same keys and disabled rules", () => {
    const menus = bar(false);
    const items = viewMenuItems("selection", menus, false);
    expect(labels(items)).toEqual(["Edit Group / Component", "-", "Delete", "Hide", "Lock", "-", "Make Group", "Make Component…", "Make Unique", "Explode", "Explode Curve", "Clear Paint"]);
    // The very items of the menu bar: what is disabled there is disabled here.
    expect(items).toContain(menus.Edit.find((it) => it !== "-" && it.label === "Make Unique"));
    expect(items.filter((it) => it !== "-" && "disabled" in it && it.disabled).map((it) => (it as { label: string }).label)).toEqual(["Edit Group / Component", "Make Unique", "Explode", "Explode Curve"]);
  });

  it("on a section plane: the three the Edit menu shows for one selected plane, and Delete", () => {
    const menus = bar(true);
    const items = viewMenuItems("section", menus, false);
    expect(labels(items)).toEqual(["Deactivate Cut", "Reverse Section Plane", "Create Group from Slice", "-", "Delete"]);
    expect(items).toContain(menus.Edit.find((it) => it !== "-" && it.label === "Delete"));
  });

  it("on empty space: selection, Zoom Extents and Undo / Redo, Undo with its step's name", () => {
    expect(labels(viewMenuItems("empty", bar(false), false))).toEqual(["Select All", "Select None", "-", "Zoom Extents", "-", "Undo Push/Pull", "Redo"]);
  });

  it("on a dimension, a guide, a sketch edge or a region: Delete alone, the menu bar's own (#139)", () => {
    const menus = bar(false);
    const items = viewMenuItems("drawn", menus, false);
    expect(labels(items)).toEqual(["Delete"]);
    expect(items[0]).toBe(menus.Edit.find((it) => it !== "-" && it.label === "Delete"));
  });

  it("on a sketch circle, arc or polygon: Delete and Explode Curve (#139)", () => {
    const menus = bar(false);
    const items = viewMenuItems("curve", menus, false);
    expect(labels(items)).toEqual(["Delete", "Explode Curve"]);
    expect(items[1]).toBe(menus.Edit.find((it) => it !== "-" && it.label === "Explode Curve"));
  });

  it("inside an open group or component, Close Group / Component is added; never a separator at an end or two together", () => {
    for (const t of ["selection", "section", "drawn", "curve", "empty"] as const) {
      const items = viewMenuItems(t, bar(true), true);
      expect(labels(items).slice(-2)).toEqual(["-", "Close Group / Component"]);
      expect(items[0]).not.toBe("-");
      expect(labels(items).join(",")).not.toContain("-,-");
    }
    // A command the menu bar does not have at the moment is left out, with its separator.
    expect(labels(viewMenuItems("section", bar(false), false))).toEqual(["Delete"]);
  });
});

describe("which list a right-click opens, from the selection it leaves (#139)", () => {
  const sketch = { kind: "sketch" as const, id: 1 };
  const solid = { kind: "object" as const, id: 2 };
  const guideOwner = { kind: "guide" as const, id: 3 };
  const dimension: Entity = { kind: "dimension", dimension: 1 };
  const guide: Entity = { kind: "guide", guide: 1 };
  const region: Entity = { kind: "region", sketch: 1, region: 1 };
  const sketchEdge: Entity = { kind: "edge", owner: sketch, a: [0, 0, 0], b: [1, 0, 0], drawn: true };
  const sketchPoint: Entity = { kind: "vertex", owner: sketch, at: [0, 0, 0] };
  const sketchCurve: Entity = { kind: "curve", owner: sketch, curve: 1 };
  /** One segment of that curve, as an edge (an edge carries its curve's id): an edge, not
   * the curve, so it gives no Explode Curve. */
  const curveSegment: Entity = { kind: "edge", owner: sketch, a: [0, 0, 0], b: [1, 0, 0], drawn: true, curve: 1 };
  const guideEdge: Entity = { kind: "edge", owner: guideOwner, a: [0, 0, 0], b: [1, 0, 0], drawn: false };
  const rim: Entity = { kind: "curve", owner: solid, curve: 2 };
  const solidEdge: Entity = { kind: "edge", owner: solid, a: [0, 0, 0], b: [1, 0, 0], drawn: false };
  const solidPoint: Entity = { kind: "vertex", owner: solid, at: [0, 0, 0] };
  const face: Entity = { kind: "face", object: 2, face: 1 };
  const object: Entity = { kind: "object", object: 2 };
  const node: Entity = { kind: "node", node: 4 };
  const section: Entity = { kind: "section", section: 1 };

  it("nothing under the pointer: empty space, whatever is selected", () => {
    expect(menuTarget(false, [])).toBe("empty");
    expect(menuTarget(false, [object])).toBe("empty");
    expect(menuTarget(false, [dimension])).toBe("empty");
  });

  it("one section plane: its own list", () => {
    expect(menuTarget(true, [section])).toBe("section");
  });

  it("a dimension, a guide, sketch edges or regions, alone or together: Delete alone", () => {
    for (const sel of [[dimension], [guide], [region], [sketchEdge], [guideEdge], [curveSegment], [dimension, guide, region, sketchEdge, curveSegment]]) {
      expect(menuTarget(true, sel), JSON.stringify(sel)).toBe("drawn");
    }
  });

  it("a sketch point keeps the list it had: Delete refuses a point alone", () => {
    expect(menuTarget(true, [sketchPoint])).toBe("selection");
    expect(menuTarget(true, [sketchEdge, sketchPoint])).toBe("selection");
  });

  it("on something with an empty selection (no path gives one): the selection's list, never Delete alone", () => {
    expect(menuTarget(true, [])).toBe("selection");
  });

  it("a sketch circle, arc or polygon among them: Delete and Explode Curve", () => {
    expect(menuTarget(true, [sketchCurve])).toBe("curve");
    expect(menuTarget(true, [dimension, sketchCurve])).toBe("curve");
    expect(menuTarget(true, [sketchEdge, sketchCurve, region])).toBe("curve");
  });

  it("anything of a solid, group or component in the selection keeps the solid's list, a solid's own rim included", () => {
    for (const sel of [[object], [node], [face], [solidEdge], [solidPoint], [rim], [dimension, object], [sketchCurve, face], [guide, rim]]) {
      expect(menuTarget(true, sel), JSON.stringify(sel)).toBe("selection");
    }
  });

  it("a section plane with something else, or two planes, keeps the list it had", () => {
    expect(menuTarget(true, [section, dimension])).toBe("selection");
    expect(menuTarget(true, [section, { kind: "section", section: 2 }])).toBe("selection");
  });

  it("against the rule before #139, over 20,000 random selections: only a solid's list changes, and only where its other items were greyed out", () => {
    /** The rule `onContextClick` had before #139. */
    const before = (on: boolean, sel: Entity[]): ViewMenuTarget => (!on ? "empty" : sel.length === 1 && sel[0].kind === "section" ? "section" : "selection");
    const pool: Entity[] = [dimension, guide, region, sketchEdge, curveSegment, sketchPoint, sketchCurve, guideEdge, rim, solidEdge, solidPoint, face, object, node, section, { kind: "section", section: 2 }];
    let seed = 20261125;
    const rnd = () => (seed = (seed * 1103515245 + 12345) % 2 ** 31) / 2 ** 31;
    const counts = { same: 0, drawn: 0, curve: 0 };
    for (let n = 0; n < 20_000; n++) {
      const on = rnd() < 0.9;
      const sel = Array.from({ length: Math.floor(rnd() * 4) }, () => pool[Math.floor(rnd() * pool.length)]);
      const now = menuTarget(on, sel);
      const was = before(on, sel);
      if (now === was) {
        counts.same++;
        continue;
      }
      const why = JSON.stringify({ on, sel });
      // Only the solid's list gives way, and only for a selection none of whose other items
      // acts on: Hide, Lock, Make Group, Make Component… and Clear Paint need `itemsOf`;
      // Edit Group / Component, Make Unique and Explode need a group or component; Explode
      // Curve needs a curve, and stays exactly when one is selected. Delete stays.
      expect(was, why).toBe("selection");
      expect(itemsOf(sel), why).toEqual([]);
      expect(sel.some((e) => e.kind === "node"), why).toBe(false);
      expect(now, why).toBe(sel.some((e) => e.kind === "curve") ? "curve" : "drawn");
      counts[now as "drawn" | "curve"]++;
    }
    expect(counts.same + counts.drawn + counts.curve).toBe(20_000);
    // Both new lists are reached often, so the comparison means something.
    expect(counts.drawn).toBeGreaterThan(500);
    expect(counts.curve).toBeGreaterThan(500);
  });
});

describe("the menu", () => {
  it("a choice runs the menu bar's action and closes it; a disabled one does nothing", () => {
    const menus = bar(false);
    const onClose = vi.fn();
    render(<ViewMenu x={10} y={10} items={viewMenuItems("selection", menus, false)} onClose={onClose} />);
    fireEvent.click(screen.getByRole("menuitem", { name: "Hide" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    const hide = menus.Edit.find((it) => it !== "-" && it.label === "Hide") as unknown as { onSelect: ReturnType<typeof vi.fn> };
    expect(hide.onSelect).toHaveBeenCalledTimes(1);
    expect((screen.getByRole("menuitem", { name: "Make Unique" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByRole("menuitem", { name: /Make Group/ }).textContent).toContain("Ctrl+G");
  });

  it("Escape, a press outside, the wheel, the window losing focus and the window resized close it, and Escape goes no further", () => {
    for (const close of [
      () => fireEvent.keyDown(window, { key: "Escape" }),
      () => fireEvent.pointerDown(document.body),
      () => fireEvent.wheel(document.body),
      () => fireEvent.blur(window),
      () => fireEvent(window, new Event("resize")),
    ]) {
      const onClose = vi.fn();
      const further = vi.fn();
      window.addEventListener("keydown", further);
      render(<ViewMenu x={10} y={10} items={viewMenuItems("empty", bar(false), false)} onClose={onClose} />);
      close();
      expect(onClose).toHaveBeenCalled();
      expect(further).not.toHaveBeenCalled();
      window.removeEventListener("keydown", further);
      cleanup();
    }
    // A left or middle press or the wheel on the view closes it and goes no further; a
    // right press there goes on (it opens the menu again where it is); a press elsewhere goes on.
    const canvas = document.createElement("canvas");
    const elsewhere = document.createElement("button");
    document.body.append(canvas, elsewhere);
    try {
      for (const [fire, reaches] of [
        [() => fireEvent.pointerDown(canvas, { button: 0 }), false],
        [() => fireEvent.pointerDown(canvas, { button: 1 }), false],
        [() => fireEvent.wheel(canvas, { deltaY: -100 }), false],
        [() => fireEvent.pointerDown(canvas, { button: 2 }), true],
        [() => fireEvent.pointerDown(elsewhere, { button: 0 }), true],
      ] as const) {
        const onClose = vi.fn();
        const heard = vi.fn();
        canvas.addEventListener("pointerdown", heard);
        canvas.addEventListener("wheel", heard);
        elsewhere.addEventListener("pointerdown", heard);
        render(<ViewMenu x={10} y={10} items={viewMenuItems("empty", bar(false), false)} onClose={onClose} view={() => canvas} />);
        fire();
        expect(onClose).toHaveBeenCalled();
        expect(heard).toHaveBeenCalledTimes(reaches ? 1 : 0);
        for (const t of [canvas, elsewhere]) for (const ev of ["pointerdown", "wheel"]) t.removeEventListener(ev, heard);
        cleanup();
      }
      // Once the menu is gone, the wheel and presses on the view reach it again.
      const heard = vi.fn();
      canvas.addEventListener("wheel", heard);
      canvas.addEventListener("pointerdown", heard);
      render(<ViewMenu x={10} y={10} items={viewMenuItems("empty", bar(false), false)} onClose={vi.fn()} view={() => canvas} />);
      cleanup();
      fireEvent.wheel(canvas, { deltaY: -100 });
      fireEvent.pointerDown(canvas, { button: 0 });
      expect(heard).toHaveBeenCalledTimes(2);
    } finally {
      canvas.remove();
      elsewhere.remove();
    }
    // A press inside it does not close it.
    const onClose = vi.fn();
    render(<ViewMenu x={10} y={10} items={viewMenuItems("empty", bar(false), false)} onClose={onClose} />);
    fireEvent.pointerDown(screen.getByRole("menuitem", { name: /Select All/ }));
    expect(onClose).not.toHaveBeenCalled();
  });
});
