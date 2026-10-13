// Lane 16 fixes: the verifier's 'chain' plugin attack (two array ops landing exactly at the
// model's 100,000 solids) froze the window for 46-54 seconds. The window's thread spent it
// listing the Outliner (each container filtered every list: quadratic) and drawing a page
// node for each of 100,000 rows, then on the cut list (100,000 rows twice: the pane and the
// print report) and on the selection highlight (each selected solid a pass over the whole
// view mesh). These replay that model's size on each piece, against a time budget far below
// what each used to take, and check what is drawn is bounded.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { CutList, CutRow, DocState, Entity, MeshData, SceneGraph } from "../kernel/types";
import { highlightFor, partsOf } from "../lib/entities";
import { contentsOf, graphIndex, objectParent, solidsIn } from "../lib/scope";
import { CutListPane, CUT_ROWS_SHOWN } from "./CutListPane";
import { OUTLINE_ROW_PX, OUTLINE_WINDOW_FROM, Outliner, outlineRows, outlineWindow } from "./Outliner";

afterEach(cleanup);

const N = 100_000;

/** The attack's model: 100,000 loose solids at the top level (and, for nesting, `groups`
 * groups holding them evenly). */
function model(n = N, groups = 0): DocState {
  const leaf = (id: number, parent: number | null) => ({ id, parent, hidden: false, locked: false, visible: true, in_component: false });
  const nodes = Array.from({ length: groups }, (_, g) => ({
    id: g + 1,
    kind: "group" as const,
    name: `G${g}`,
    label: `G${g}`,
    definition: null,
    parent: null,
    tag: null,
    hidden: false,
    locked: false,
    visible: true,
    xform: { linear: [[1, 0, 0], [0, 1, 0], [0, 0, 1]], translation: [0, 0, 0] },
    in_component: false,
  }));
  const ids = Array.from({ length: n }, (_, i) => 1000 + i);
  return {
    tags: {},
    object_info: ids.map((id) => ({ id, name: null, tag: null, material: null })),
    solids: ids.map((id, k) => ({ id, number: k + 1, min: [0, 0, 0], max: [1, 1, 1], volume: 1 })),
    meshes: [],
    scenes: [],
    graph: {
      context: null,
      path: [],
      nodes,
      objects: ids.map((id, k) => leaf(id, groups ? (k % groups) + 1 : null)),
      meshes: [],
      sketches: [],
      definitions: [],
      tag_colors: {},
    },
  } as unknown as DocState;
}

/** A view mesh with six faces (one part each) and twelve edges per solid. */
function mesh(state: DocState): MeshData {
  const ids = state.graph!.objects.map((o) => o.id);
  const parts = ids.flatMap((id) => Array.from({ length: 6 }, (_, f) => ({ kind: "face" as const, owner: id, index: f })));
  const edgeOwners = ids.map((id) => ({ kind: "object" as const, id }));
  const edgeOwner = new Uint32Array(ids.length * 12).map((_, i) => Math.floor(i / 12));
  return { parts, edgeOwners, edgeOwner, edges: new Float32Array(ids.length * 12 * 6), faceIds: new Uint32Array(0), positions: new Float32Array(0), indices: new Uint32Array(0) } as unknown as MeshData;
}

const ms = (f: () => void) => {
  const t = performance.now();
  f();
  return performance.now() - t;
};

describe("a model at the 100,000-solid limit", () => {
  it("the Outliner's rows are worked out in linear time", () => {
    const st = model();
    let rows: ReturnType<typeof outlineRows> = [];
    const t = ms(() => (rows = outlineRows(st, new Set())));
    expect(rows.length).toBe(N);
    // Was 21 s (the dev build; each container filtered every list). Now tens of ms.
    expect(t).toBeLessThan(1500);
    // In groups, unfolded: every solid listed under its group, in the graph's order.
    const g = model(N, 100);
    const open = outlineRows(g, new Set(Array.from({ length: 100 }, (_, k) => k + 1)));
    expect(open.length).toBe(N + 100);
    expect(open[0].key).toBe("n:1");
    expect(open[1].key).toBe("o:1000");
    expect(open[2].key).toBe("o:1100");
  });

  it("the Outliner draws the rows in view, not 100,000 page nodes, and keeps its scroll height", () => {
    const st = model();
    const t = ms(() => {
      render(<Outliner state={st} selection={[]} onSelect={() => undefined} onAction={() => undefined} onRename={() => undefined} />);
    });
    const items = screen.getAllByRole("treeitem");
    expect(items.length).toBeLessThan(400);
    expect(items[0].getAttribute("aria-setsize")).toBe(String(N));
    expect(items[0].getAttribute("aria-posinset")).toBe("1");
    const list = screen.getByRole("tree");
    expect(list.getAttribute("data-rows")).toBe(String(N));
    const drawn = items.length;
    expect(parseFloat(list.style.paddingBottom)).toBe((N - drawn) * OUTLINE_ROW_PX);
    expect(t).toBeLessThan(5000);
  });

  it("a short tree is drawn whole, as before", () => {
    const st = model(OUTLINE_WINDOW_FROM);
    render(<Outliner state={st} selection={[]} onSelect={() => undefined} onAction={() => undefined} onRename={() => undefined} />);
    expect(screen.getAllByRole("treeitem").length).toBe(OUTLINE_WINDOW_FROM);
    expect(screen.getByRole("tree").style.paddingTop).toBe("");
  });

  it("the window of rows follows the scroll position, with a screenful either side", () => {
    expect(outlineWindow(N, 0, 600)).toEqual({ first: 0, last: Math.ceil(1200 / OUTLINE_ROW_PX) });
    const mid = outlineWindow(N, 240_000, 600);
    expect(mid.first).toBe(Math.floor((240_000 - 600) / OUTLINE_ROW_PX));
    expect(mid.last).toBe(Math.ceil((240_000 + 1200) / OUTLINE_ROW_PX));
    expect(outlineWindow(N, N * OUTLINE_ROW_PX + 5000, 600).last).toBe(N);
    // A hidden pane (no height) still draws a screenful.
    expect(outlineWindow(N, 0, 0).last).toBeGreaterThan(40);
  });

  it("the scene graph's lookups are built once per graph: a parent, a node's contents", () => {
    const st = model(N, 1000);
    const g = st.graph as SceneGraph;
    const t = ms(() => {
      for (const o of g.objects) objectParent(g, o.id);
    });
    expect(t).toBeLessThan(1000);
    expect(graphIndex(g)).toBe(graphIndex(g));
    expect(solidsIn(g, 1)).toEqual(Array.from({ length: 100 }, (_, k) => 1000 + k * 1000));
    const all = contentsOf(g, Array.from({ length: 1000 }, (_, k) => k + 1));
    expect(all.objects.size).toBe(N);
  });

  it("selecting all 100,000 solids is highlighted in a pass or two over the view mesh, not one per solid", () => {
    const st = model();
    const m = mesh(st);
    const sel: Entity[] = st.graph!.objects.map((o) => ({ kind: "object", object: o.id }));
    let h: ReturnType<typeof highlightFor> | null = null;
    const t = ms(() => (h = highlightFor(sel, null, m, st.graph)));
    expect(h!.selectedParts.length).toBe(N * 6);
    expect(h!.selectedEdges.length).toBe(N * 12);
    // Was 10 s at a tenth of this size (each solid a pass over the mesh); and a spread of
    // a million edges into push() threw past the call stack's limit.
    expect(t).toBeLessThan(3000);
    // All of it as groups: the same parts, through the nodes.
    const gs = model(N, 100);
    const groupSel: Entity[] = Array.from({ length: 100 }, (_, k) => ({ kind: "node", node: k + 1 }));
    const hg = highlightFor(groupSel, null, mesh(gs), gs.graph);
    expect(hg.selectedParts.length).toBe(N * 6);
    expect(partsOf({ kind: "node", node: 1 }, mesh(gs), gs.graph).length).toBe(1000 * 6);
  });
});

describe("the cut list of 100,000 parts", () => {
  const row = (i: number): CutRow =>
    ({
      key: { object: 1000 + i },
      name: `Solid ${i + 1}`,
      qty: 1,
      total: 1,
      hidden: 0,
      steps: [12, 96, 96 + i],
      grain: "length",
      material: null,
      sheet: false,
      occurrences: [{ item: { object: 1000 + i }, visible: true }],
      board_feet: 0.5,
    }) as unknown as CutRow;
  const list = (n: number): CutList =>
    ({
      rows: Array.from({ length: n }, (_, i) => row(i)),
      rounding: "sixteenth",
      visible_only: false,
      totals: { parts: n, pieces: n, board_feet: n * 0.5, sheet_count: 0, lumber: [], sheets: [], hidden: 0, loose_faces: 0, loose_here: 0, loose_places: [] },
    }) as unknown as CutList;
  const props = (l: CutList) => ({
    list: l,
    rounding: "sixteenth" as const,
    onRounding: () => undefined,
    visibleOnly: false,
    onVisibleOnly: () => undefined,
    selected: new Set<string>(),
    onSelectRows: () => undefined,
    onGrain: () => undefined,
    onStock: () => undefined,
    onCsv: () => undefined,
    onPdf: () => undefined,
    onPrint: () => undefined,
    onLabels: () => undefined,
  });

  it("draws its first rows and more on request; the totals cover every row", () => {
    const l = list(N);
    render(<CutListPane {...props(l)} />);
    const table = screen.getByRole("table", { name: "Cut list" });
    expect(table.querySelectorAll("tbody tr").length).toBe(CUT_ROWS_SHOWN);
    expect(screen.getByTestId("cutlist-more").textContent).toContain(`${N - CUT_ROWS_SHOWN} more rows`);
    act(() => {
      fireEvent.click(screen.getByRole("button", { name: `Show ${CUT_ROWS_SHOWN} more` }));
    });
    expect(table.querySelectorAll("tbody tr").length).toBe(2 * CUT_ROWS_SHOWN);
  });

  it("a list that fits is drawn whole, with no note", () => {
    render(<CutListPane {...props(list(12))} />);
    expect(screen.getByRole("table", { name: "Cut list" }).querySelectorAll("tbody tr").length).toBe(12);
    expect(screen.queryByTestId("cutlist-more")).toBeNull();
  });
});
