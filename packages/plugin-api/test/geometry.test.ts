import { describe, expect, it } from "vitest";
import { bbox, plane, transform, vec3, type Vec3 } from "../src/geometry.ts";

const close = (a: Vec3, b: Vec3) => a.forEach((x, i) => expect(x).toBeCloseTo(b[i], 12));

describe("vec3", () => {
  it("does the arithmetic", () => {
    expect(vec3.add([1, 2, 3], [4, 5, 6])).toEqual([5, 7, 9]);
    expect(vec3.cross([1, 0, 0], [0, 1, 0])).toEqual([0, 0, 1]);
    expect(vec3.dot([1, 2, 3], [4, 5, 6])).toBe(32);
    expect(vec3.normalize([0, 0, 0])).toEqual([0, 0, 0]);
    expect(vec3.length(vec3.perpendicular([0, 0, 2]))).toBeCloseTo(1);
    expect(vec3.dot(vec3.perpendicular([1, 2, 3]), [1, 2, 3])).toBeCloseTo(0);
  });
});

describe("plane", () => {
  it("projects, measures and frames", () => {
    const pl = plane.fromPointNormal([0, 0, 1], [0, 0, 5]);
    expect(plane.distance(pl, [3, 4, 3])).toBe(2);
    expect(plane.project(pl, [3, 4, 3])).toEqual([3, 4, 1]);
    const side = plane.fromPointNormal([0, 0, 0], [1, 0, 0]);
    const f = plane.frame(side);
    expect(f.u[2]).toBeCloseTo(0);
    close(vec3.cross(f.u, f.v), side.normal);
    expect(plane.fromPoints([0, 0, 0], [1, 0, 0], [2, 0, 0])).toBeNull();
    close(plane.intersectLine(pl, [0, 0, 0], [0, 0, 1])!, [0, 0, 1]);
  });
});

describe("bbox", () => {
  it("grows and measures", () => {
    const b = bbox.fromPoints([[1, 2, 3], [-1, 5, 0]]);
    expect(b).toEqual({ min: [-1, 2, 0], max: [1, 5, 3] });
    expect(bbox.size(b)).toEqual([2, 3, 3]);
    expect(bbox.isEmpty(bbox.empty())).toBe(true);
    expect(bbox.size(bbox.empty())).toEqual([0, 0, 0]);
    expect(bbox.corners(b)).toHaveLength(8);
    expect(bbox.contains(b, [0, 3, 1])).toBe(true);
  });
});

describe("transform", () => {
  it("rotates about a point, composes and inverts", () => {
    const r = transform.rotation([0, 0, 1], 90, [1, 0, 0]);
    close(transform.apply(r, [2, 0, 0]), [1, 1, 0]);
    const s = transform.scale(2, [1, 1, 1]);
    close(transform.apply(s, [2, 1, 1]), [3, 1, 1]);
    const m = transform.compose(transform.translation([0, 0, 5]), r);
    close(transform.apply(m, [2, 0, 0]), [1, 1, 5]);
    const inv = transform.invert(m)!;
    close(transform.apply(inv, [1, 1, 5]), [2, 0, 0]);
    expect(transform.invert(transform.scale([1, 0, 1]))).toBeNull();
    expect(transform.determinant(transform.scale([2, 3, 4]))).toBeCloseTo(24);
  });
});
