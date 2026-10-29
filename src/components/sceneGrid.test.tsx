// A scene remembers the ground grid (lane 26, issue #6): what the app saves with a scene
// (always the grid that shows, even under the default style), what going to a scene says,
// and, through the real kernel (WASM, as the worker runs it), that going to a scene saved
// with the grid hidden unchecks View > Grid and the Styles pane's Ground grid, and going to
// one saved with it shown checks them again. The WASM part needs `npm run wasm`; it is
// skipped without the kernel unless REQUIRE_KERNEL is set.
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { sceneLooksOf, sceneNote } from "../App";
import type { DocState, SceneDef, WasmDoc } from "../kernel/types";
import { compactStyle, fullStyle, gridAfterScene, gridShown } from "../lib/looks";
import { StylesPane } from "./StylesPane";

afterEach(cleanup);

describe("a scene and the grid (lane 26, issue #6)", () => {
  it("a scene saves the grid that shows, under the default style too", () => {
    expect(sceneLooksOf({ style: null, shadows: null })).toEqual({ style: null, shadows: null, grid: true });
    expect(sceneLooksOf({})).toEqual({ style: null, shadows: null, grid: true });
    expect(sceneLooksOf({ style: { grid: false } })).toEqual({ style: { grid: false }, shadows: null, grid: false });
    expect(sceneLooksOf({ style: { face: "x_ray" } }).grid).toBe(true);
  });

  it("the grid after going to a scene: its switch, else its style's, else as it is", () => {
    expect(gridAfterScene({ grid: true }, { grid: false })).toBe(true);
    expect(gridAfterScene({ grid: false, style: null }, null)).toBe(false);
    // The switch wins over a style that disagrees (the kernel applies it over the style).
    expect(gridAfterScene({ grid: false, style: { grid: true } }, null)).toBe(false);
    // A lane-10 scene: its style's grid.
    expect(gridAfterScene({ style: { grid: false } }, null)).toBe(false);
    expect(gridAfterScene({ style: { face: "x_ray" } }, { grid: false })).toBe(true);
    // An older or imported scene: the grid as it is.
    expect(gridAfterScene({}, { grid: false })).toBe(false);
    expect(gridAfterScene({ style: null }, null)).toBe(true);
    expect(gridShown(null)).toBe(true);
  });

  it("going to a scene says the grid only when it changed (lane 10's wording)", () => {
    expect(sceneNote("Front", { grid: false }, null)).toBe("Scene 'Front': grid hidden.");
    expect(sceneNote("Front", { grid: true }, { grid: false })).toBe("Scene 'Front': grid shown.");
    expect(sceneNote("Front", { grid: true }, null)).toBe("Scene 'Front'.");
    expect(sceneNote("Front", {}, { grid: false })).toBe("Scene 'Front'.");
    expect(sceneNote("Front", { hidden_tags: ["Legs", "Rails"], grid: false }, null)).toBe("Scene 'Front': hides 'Legs', 'Rails'; grid hidden.");
    expect(sceneNote("Front", { hidden_tags: ["Legs"], grid: false }, { grid: false })).toBe("Scene 'Front': hides 'Legs'.");
  });
});

const pkg = path.resolve(__dirname, "../wasm/pkg");
const built = existsSync(path.join(pkg, "sawhorse_wasm_bg.wasm"));
type Mod = { initSync(o: { module: Buffer }): void; document_new(): WasmDoc };
async function kernel(): Promise<Mod> {
  const mod = (await import(/* @vite-ignore */ path.join(pkg, "sawhorse_wasm.js"))) as Mod;
  mod.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
  return mod;
}
const run = built || process.env.REQUIRE_KERNEL ? describe : describe.skip;

run("a scene and the grid through the WASM kernel (lane 26, issue #6)", () => {
  const state = (d: WasmDoc) => JSON.parse(d.state_json()) as DocState;
  /** View > Grid (src/App.tsx `setGrid`): the style with the switch flipped, one op. */
  const setGrid = (d: WasmDoc, on: boolean) => d.apply_op(JSON.stringify({ op: "set_style", style: compactStyle({ ...fullStyle(state(d).style), grid: on }) }));
  /** + on the scene tabs (src/App.tsx `sceneHere`, the camera aside). */
  const addScene = (d: WasmDoc, name: string) => {
    const scene: SceneDef = { name, eye: [3, -3, 2], target: [0, 0, 0], up: [0, 0, 1], fov: 35, parallel: false, hidden_tags: [], context: null, ...sceneLooksOf(state(d)) };
    d.apply_op(JSON.stringify({ op: "add_scene", scene }));
  };
  const go = (d: WasmDoc, name: string) => d.apply_op(JSON.stringify({ op: "activate_scene", name }));
  const paneChecked = (d: WasmDoc) => {
    cleanup();
    render(<StylesPane style={state(d).style ?? null} shadows={null} onStyle={vi.fn()} onShadows={vi.fn()} onPreviewShadows={vi.fn()} />);
    return (screen.getByLabelText("Ground grid") as HTMLInputElement).checked;
  };

  it("the issue's steps: Scene 1 unchecks the grid, Scene 2 (the default style) checks it again", async () => {
    const d = (await kernel()).document_new();
    setGrid(d, false);
    addScene(d, "Scene 1");
    setGrid(d, true);
    expect(state(d).style ?? null).toBeNull();
    addScene(d, "Scene 2");
    const scenes = state(d).scenes;
    expect(scenes.map((s) => [s.name, s.style ? gridShown(s.style) : null, s.grid])).toEqual([
      ["Scene 1", false, false],
      ["Scene 2", null, true],
    ]);
    const hash = d.document_hash();

    go(d, "Scene 1");
    // View > Grid's checkmark reads gridShown(st.style); the Styles pane its checkbox.
    expect(gridShown(state(d).style)).toBe(false);
    expect(paneChecked(d)).toBe(false);
    go(d, "Scene 2");
    expect(gridShown(state(d).style)).toBe(true);
    expect(paneChecked(d)).toBe(true);
    expect(state(d).style ?? null).toBeNull();
    expect(d.document_hash()).toBe(hash);

    // One undo step: back to the hidden grid of Scene 1.
    d.undo();
    expect(gridShown(state(d).style)).toBe(false);
    expect(paneChecked(d)).toBe(false);
  });

  it("a scene saved with another style and the grid hidden brings both back", async () => {
    const d = (await kernel()).document_new();
    d.apply_op(JSON.stringify({ op: "set_style", style: { preset: "X-ray", face: "x_ray" } }));
    setGrid(d, false);
    addScene(d, "X-ray");
    d.apply_op(JSON.stringify({ op: "set_style", style: null }));
    go(d, "X-ray");
    expect(state(d).style).toMatchObject({ preset: "X-ray", face: "x_ray", grid: false });
    expect(paneChecked(d)).toBe(false);
  });
});
