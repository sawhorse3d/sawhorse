// #129: the resolver folds a dimension's number and extension lines into the kernel's pick,
// for the tools that ask (Select, Erase), and for no other tool. #130: where a click ends
// and a drag begins.
import { describe, expect, it } from "vitest";
import type { KernelClient } from "../kernel/client";
import type { Inference, V3 } from "../kernel/types";
import type { ViewportEngine } from "../viewport/engine";
import type { ViewportPointer } from "../components/Viewport";
import { ToolDriver, kernelResolver, type Resolver } from "./driver";
import { fakeHost } from "./fakeHost";
import { RectTool } from "./rect";

const MM = 0.001;
// A Top view, orthographic: 1 mm a pixel, the origin at canvas (500, 400).
const engine = {
  ray: (x: number, y: number) => ({ origin: [(x - 500) * MM, (400 - y) * MM, 10] as V3, dir: [0, 0, -1] as V3 }),
  pixelScale: () => ({ ortho: true, scale: MM, eye: [0, 0, 10] as V3, forward: [0, 0, -1] as V3 }),
  worldToScreen: (p: V3): [number, number] => [500 + p[0] / MM, 400 - p[1] / MM],
} as unknown as ViewportEngine;
// The kernel sees a face under every point (the dimension lies over a solid's top).
const face = { entity: { kind: "face", object: 1, face: 2 }, point: [0, 0, 0], surface: null };
const kernel = { infer: async () => ({ kind: "on-face", point: [0, 0, 0], tip: "On Face", entity: null, lines: [], surface: null, picked: face }) as unknown as Inference } as unknown as KernelClient;
const dims = [{ id: 7, a: [0, 0, 0] as V3, b: [0.3, 0, 0] as V3, offset: [0, -0.1, 0] as V3, length: 0.3 }];

describe("kernelResolver and dimensions", () => {
  const resolve = kernelResolver(
    () => kernel,
    () => engine,
    () => [0.0254, 0.3048],
    undefined,
    undefined,
    () => ({ dims, text: () => "300", size: () => [30, 18] }),
  );
  const mods = { shift: false, ctrl: false, alt: false };

  it("with pickDimensions, the number and an extension line over a face pick the dimension", async () => {
    expect((await resolve(650, 508, { pickDimensions: true }, mods))?.picked?.entity).toEqual({ kind: "dimension", dimension: 7 });
    expect((await resolve(502, 450, { pickDimensions: true }, mods))?.picked?.entity).toEqual({ kind: "dimension", dimension: 7 });
    // Away from it: the face, as the kernel said.
    expect((await resolve(400, 300, { pickDimensions: true }, mods))?.picked?.entity).toEqual(face.entity);
  });

  it("without it (the draw tools, Move, Push/Pull), the kernel's pick is untouched", async () => {
    expect((await resolve(650, 508, {}, mods))?.picked?.entity).toEqual(face.entity);
    expect((await resolve(502, 450, {}, mods))?.picked?.entity).toEqual(face.entity);
  });
});

// #130: the line between a click and a drag, through the real pointer path (ToolDriver):
// a press and release 4 px apart is a click, 5 px apart a drag.

describe("a click or a drag, by how far the pointer went", () => {
  // Every pixel lands on the ground (a Top view, 1 cm a pixel).
  const ground: Resolver = async (x, y) => ({
    ray: { origin: [x * 0.01, -y * 0.01, 10], dir: [0, 0, -1] },
    picked: null,
    snap: { point: [x * 0.01, -y * 0.01, 0], kind: "free", tip: "", surface: null, entity: null, lines: [] },
  });
  const p = (type: ViewportPointer["type"], x: number, y: number): ViewportPointer => ({ type, x, y, button: 0, shift: false, ctrl: false, alt: false });
  async function press(dx: number, dy: number) {
    const h = fakeHost();
    const t = new RectTool(h);
    const d = new ToolDriver(ground, () => t);
    d.pointer(p("move", 100, 100));
    d.pointer(p("down", 100, 100));
    d.pointer(p("move", 100 + dx, 100 + dy));
    d.pointer(p("up", 100 + dx, 100 + dy));
    await d.idle();
    await d.idle();
    return { h, t };
  }

  it("4 px apart is a click: the first corner is placed, no drag words", async () => {
    for (const [dx, dy] of [
      [4, 0],
      [0, -4],
      [2.8, 2.8],
    ]) {
      const { h, t } = await press(dx, dy);
      expect(t.busy(), `${dx},${dy}`).toBe(true);
      expect(h.statuses.at(-1)?.text).toMatch(/^First corner set/);
    }
  });

  it("5 px apart is a drag: no point, and the drag words", async () => {
    for (const [dx, dy] of [
      [5, 0],
      [0, 5],
      [3, 4],
    ]) {
      const { h, t } = await press(dx, dy);
      expect(t.busy(), `${dx},${dy}`).toBe(false);
      expect(h.statuses.at(-1)).toEqual({ text: "Click, move, then click again. A drag does not draw.", error: true });
    }
  });
});
