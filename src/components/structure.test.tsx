import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

afterEach(cleanup);
import type { DocState, GraphNode, SceneDef } from "../kernel/types";
import { itemsOf, nextName } from "../App";
import { NameDialog } from "./NameDialog";
import { Outliner, outlineRows } from "./Outliner";
import { SceneTabs } from "./SceneTabs";
import { TagsPane, hexToRgb, tagCounts } from "./TagsPane";
import { entityShown } from "../lib/scope";

const I = { linear: [[1, 0, 0], [0, 1, 0], [0, 0, 1]], translation: [0, 0, 0] } as GraphNode["xform"];
const node = (id: number, kind: "group" | "instance", parent: number | null, label: string, definition: number | null = null, extra: Partial<GraphNode> = {}): GraphNode => ({
  id,
  kind,
  name: kind === "group" ? label : null,
  label,
  definition,
  parent,
  tag: null,
  hidden: false,
  locked: false,
  visible: true,
  xform: I,
  in_component: false,
  ...extra,
});

// The bookshelf: group Carcass (1) with two Side instances (2, 3); four Shelf instances
// (4..7) at the top level, one hidden; a loose solid 15.
function state(context: number | null = null): DocState {
  const leaf = (id: number, parent: number | null) => ({ id, parent, hidden: false, locked: false, visible: true });
  return {
    tags: { Carcass: true, Shelves: false },
    object_info: [10, 11, 12, 13, 14, 15, 16].map((id) => ({ id, name: id === 16 ? "Back" : null, tag: id >= 12 && id <= 15 ? "Shelves" : null, material: null })),
    solids: [10, 11, 12, 13, 14, 15, 16].map((id, k) => ({ id, number: k + 1, min: [0, 0, 0], max: [1, 1, 1], volume: 1 })),
    meshes: [],
    scenes: [],
    graph: {
      context,
      path: context === null ? [] : context === 2 ? [1, 2] : [context],
      nodes: [
        node(1, "group", null, "Carcass", null, { tag: "Carcass" }),
        node(2, "instance", 1, "Side", 1),
        node(3, "instance", 1, "Side", 1),
        node(4, "instance", null, "Shelf", 2),
        node(5, "instance", null, "Shelf", 2),
        node(6, "instance", null, "Shelf", 2),
        node(7, "instance", null, "Shelf", 2, { hidden: true, visible: false }),
      ],
      objects: [leaf(10, 2), leaf(11, 3), leaf(12, 4), leaf(13, 5), leaf(14, 6), leaf(15, 7), leaf(16, null)],
      meshes: [],
      sketches: [[1, null]],
      definitions: [
        { id: 1, name: "Side", instances: 2, solids: 1, min: [0, 0, 0], max: [1, 1, 1] },
        { id: 2, name: "Shelf", instances: 4, solids: 1, min: [0, 0, 0], max: [1, 1, 1] },
      ],
      tag_colors: { Carcass: "#c48a2a" },
    },
  } as unknown as DocState;
}

describe("Outliner", () => {
  it("lists the tree with kind hints, folded except along the open context", () => {
    const rows = outlineRows(state(), new Set());
    expect(rows.map((r) => [r.label, r.hint, r.level])).toEqual([
      ["Carcass", "group · Carcass", 0],
      ["Shelf", "component · 1 of 4", 0],
      ["Shelf", "component · 2 of 4", 0],
      ["Shelf", "component · 3 of 4", 0],
      ["Shelf", "component · 4 of 4 · hidden", 0],
      ["Back", "", 0],
    ]);
    const open = outlineRows(state(2), new Set([1, 2]));
    expect(open.slice(0, 4).map((r) => [r.label, r.level, r.open])).toEqual([
      ["Carcass", 0, true],
      ["Side", 1, true],
      ["Solid 1", 2, false],
      ["Side", 1, false],
    ]);
  });

  it("selects, toggles the eye and the lock, renames, and offers the context menu", () => {
    const onSelect = vi.fn();
    const onAction = vi.fn();
    const onRename = vi.fn();
    render(<Outliner state={state()} selection={[{ kind: "node", node: 4 }]} onSelect={onSelect} onAction={onAction} onRename={onRename} />);
    const row = (label: string, k = 0) => screen.getAllByText(label)[k].closest("li")!;
    expect(row("Shelf").getAttribute("aria-selected")).toBe("true");
    fireEvent.click(row("Carcass"));
    expect(onSelect).toHaveBeenLastCalledWith(expect.objectContaining({ key: "n:1" }), "replace");
    fireEvent.click(row("Shelf", 1), { shiftKey: true });
    expect(onSelect).toHaveBeenLastCalledWith(expect.objectContaining({ key: "n:5" }), "add");
    fireEvent.click(screen.getByRole("button", { name: "Hide Carcass" }));
    expect(onAction).toHaveBeenLastCalledWith("hide", [expect.objectContaining({ key: "n:1" })]);
    fireEvent.click(screen.getAllByRole("button", { name: "Show Shelf" })[0]);
    expect(onAction).toHaveBeenLastCalledWith("show", [expect.objectContaining({ key: "n:7" })]);
    fireEvent.click(screen.getByRole("button", { name: "Lock Back" }));
    expect(onAction).toHaveBeenLastCalledWith("lock", [expect.objectContaining({ key: "o:16" })]);
    // Unfold the carcass: its sides show.
    fireEvent.click(screen.getByRole("button", { name: "Unfold Carcass" }));
    expect(screen.getAllByText("Side")).toHaveLength(2);
    // Rename on double-click: the old name starts selected, so typing replaces it.
    fireEvent.doubleClick(screen.getByText("Back"));
    const input = screen.getByLabelText("New name for Back") as HTMLInputElement;
    expect(document.activeElement).toBe(input);
    expect([input.selectionStart, input.selectionEnd]).toEqual([0, "Back".length]);
    fireEvent.change(input, { target: { value: "Back panel" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onRename).toHaveBeenLastCalledWith(expect.objectContaining({ key: "o:16" }), "Back panel");
    // Right-click the selected shelf: the menu acts on it.
    fireEvent.contextMenu(row("Shelf"));
    const menu = screen.getByRole("menu", { name: "Outliner actions" });
    expect(menu).toBeTruthy();
    expect((screen.getByRole("menuitem", { name: "Make Unique" }) as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(screen.getByRole("menuitem", { name: /Explode/ }));
    expect(onAction).toHaveBeenLastCalledWith("explode", [expect.objectContaining({ key: "n:4" })]);
    fireEvent.contextMenu(row("Back"));
    expect((screen.getByRole("menuitem", { name: /Explode/ }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("menuitem", { name: /Make Component/ }));
    expect(onAction).toHaveBeenLastCalledWith("component", [expect.objectContaining({ key: "o:16" })]);
  });
});

describe("Tags pane", () => {
  it("lists Untagged first, then the tags with colours, counts and visibility", () => {
    const calls: unknown[][] = [];
    const rec =
      (name: string) =>
      (...a: unknown[]) =>
        calls.push([name, ...a]);
    render(<TagsPane state={state()} canTag onCreate={rec("create")} onVisible={rec("visible")} onColor={rec("color")} onRename={rec("rename")} onDelete={rec("delete")} onAssign={rec("assign")} />);
    const rows = [...document.querySelectorAll(".tree.tags li")].map((li) => li.getAttribute("data-tag"));
    expect(rows).toEqual(["", "Carcass", "Shelves"]);
    expect(screen.getByText("default · 9 items")).toBeTruthy(); // 3 solids and 6 components untagged
    expect(screen.getByText("hidden · 4 items")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Show tag Shelves" }));
    fireEvent.click(screen.getByRole("button", { name: "Put the selection on Carcass" }));
    fireEvent.click(screen.getByRole("button", { name: "Put the selection on Untagged" }));
    fireEvent.change(screen.getByLabelText("Colour of Carcass"), { target: { value: "#112233" } });
    fireEvent.doubleClick(screen.getByText("Carcass", { selector: ".label" }));
    const input = screen.getByLabelText("New name for tag Carcass");
    fireEvent.change(input, { target: { value: "Case" } });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.click(screen.getByRole("button", { name: "Delete tag Shelves" }));
    fireEvent.click(screen.getByRole("button", { name: /New tag/ }));
    expect(calls).toEqual([
      ["visible", "Shelves", true],
      ["assign", "Carcass"],
      ["assign", null],
      ["color", "Carcass", "#112233"],
      ["rename", "Carcass", "Case"],
      ["delete", "Shelves"],
      ["create"],
    ]);
    expect(hexToRgb("#112233")).toEqual([17, 34, 51]);
    expect(tagCounts(state()).get("Shelves")).toBe(4);
  });
});

describe("Scene tabs", () => {
  const scene = (name: string): SceneDef => ({ name, eye: [1, -1, 1], target: [0, 0, 0], up: [0, 0, 1], fov: 35, parallel: true, hidden_tags: [], context: null });
  it("goes to, adds, updates, renames and deletes scenes", () => {
    const calls: unknown[][] = [];
    const rec =
      (name: string) =>
      (...a: unknown[]) =>
        calls.push([name, ...a]);
    render(<SceneTabs scenes={[scene("Front"), scene("Iso")]} active="Iso" onGo={rec("go")} onAdd={rec("add")} onUpdate={rec("update")} onRename={rec("rename")} onDelete={rec("delete")} />);
    expect(screen.getByRole("tab", { name: "Iso" }).getAttribute("aria-selected")).toBe("true");
    fireEvent.click(screen.getByRole("tab", { name: "Front" }));
    fireEvent.click(screen.getByRole("button", { name: /Add a scene/ }));
    fireEvent.contextMenu(screen.getByRole("tab", { name: "Front" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Update/ }));
    fireEvent.contextMenu(screen.getByRole("tab", { name: "Iso" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    fireEvent.doubleClick(screen.getByRole("tab", { name: "Front" }));
    const input = screen.getByLabelText("New name for scene Front");
    fireEvent.change(input, { target: { value: "Elevation" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(calls).toEqual([["go", "Front"], ["add"], ["update", "Front"], ["delete", "Iso"], ["rename", "Front", "Elevation"]]);
  });
});

describe("helpers", () => {
  it("turns a selection into items and names things", () => {
    expect(
      itemsOf([
        { kind: "face", object: 3, face: 2 },
        { kind: "object", object: 3 },
        { kind: "node", node: 5 },
        { kind: "edge", owner: { kind: "sketch", id: 1 }, a: [0, 0, 0], b: [1, 0, 0], drawn: true },
      ]),
    ).toEqual([{ object: 3 }, { node: 5 }]);
    expect(nextName("Scene", ["Scene 1", "Scene 3"])).toBe("Scene 4");
    expect(nextName("Tag", [])).toBe("Tag 1");
  });

  it("the naming dialog commits on Enter and cancels on Esc", () => {
    const onOk = vi.fn();
    const onClose = vi.fn();
    render(<NameDialog req={{ title: "Make Component", label: "Name", value: "Component 1", ok: "Make Component", onOk }} onClose={onClose} />);
    const input = screen.getByLabelText("Name");
    fireEvent.change(input, { target: { value: "Shelf" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onOk).toHaveBeenCalledWith("Shelf");
    expect(onClose).toHaveBeenCalled();
  });
});

describe("Selection and visibility", () => {
  it("what is hidden (itself, by a tag or a hidden parent) is not shown, so it leaves the selection", () => {
    const st = state();
    expect(entityShown({ kind: "node", node: 4 }, st)).toBe(true);
    expect(entityShown({ kind: "node", node: 7 }, st)).toBe(false);
    const hidden = { ...st, graph: { ...st.graph, objects: st.graph.objects.map((o) => (o.id === 15 ? { ...o, visible: false } : o)) } } as DocState;
    expect(entityShown({ kind: "object", object: 15 }, hidden)).toBe(false);
    expect(entityShown({ kind: "face", object: 15, face: 2 }, hidden)).toBe(false);
    expect(entityShown({ kind: "edge", owner: { kind: "object", id: 15 }, a: [0, 0, 0], b: [1, 0, 0], drawn: false }, hidden)).toBe(false);
    expect(entityShown({ kind: "face", object: 16, face: 2 }, hidden)).toBe(true);
    expect(entityShown({ kind: "node", node: 99 }, st)).toBe(false);
  });
});
