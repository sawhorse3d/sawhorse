// The sheet canvas (lane 12): the paper as the kernel lays it out, zoomed and panned, with
// the select tool (click, Shift+click, box, drag to move on the grid, corner handles to
// resize, Alt+drag inside a viewport to slide its drawing, drag a dimension to stand it
// off) and the placement tools (viewport, dimension, label, table, image, line,
// rectangle). It never edits anything itself: every change is a request the workspace
// turns into one kernel op, so the kernel stays the judge and every edit is one undo step.
import { forwardRef, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { DimKind, ItemKind, LeaderEnd, Sheet } from "../kernel/sheetTypes";
import type { V3 } from "../kernel/types";
import { itemRect, rectFrom } from "./paper";
import { CANVAS_K, dimOffset, dimPreview, hitItem, modelToPaper, sheetLayers, snapPoint, type Href, type ItemInfo, type SheetRender, type SheetSnap, type ViewportInfo } from "./render";

export type SheetToolId = "select" | "viewport" | "dimension" | "label" | "table" | "image" | "line" | "rect";

type P2 = [number, number];
type Rect = [number, number, number, number];

/** What a placement tool asks the workspace to make. */
export type PlaceRequest =
  | { kind: "box"; tool: "viewport" | "table" | "image" | "rect"; rect: Rect; dragged: boolean }
  | { kind: "line"; a: P2; b: P2 }
  | { kind: "label"; at: P2; leader: LeaderEnd | null; text: string }
  | { kind: "dimension"; viewport: number; dim: DimKind; points: V3[]; offset: number };

export interface SheetCanvasProps {
  render: SheetRender | null;
  sheet: Sheet | null;
  href: Href;
  tool: SheetToolId;
  dimKind: DimKind;
  selection: number[];
  /** Snap to the sheet's grid. */
  gridSnap: boolean;
  onSelection: (ids: number[]) => void;
  /** Where a paper point snaps on viewport `item`'s drawing (kernel inference). */
  snap: (item: number, at: P2, radius: number) => Promise<SheetSnap | null>;
  onMove: (ids: number[], by: P2) => void;
  onSet: (id: number, kind: ItemKind) => void;
  onPlace: (req: PlaceRequest) => void;
  onMessage: (text: string, error?: boolean) => void;
}

export interface SheetCanvasHandle {
  /** Esc: drop what a tool is in the middle of (true when there was something). */
  cancel(): boolean;
  fit(): void;
  /** Client (page) coordinates of a paper point (tests and captures). */
  toClient(p: P2): P2 | null;
}

/** A pending multi-click placement. */
type Pending =
  | { t: "dim"; viewport: number; points: V3[]; paper: P2[] }
  | { t: "label-tip"; tip: P2; leader: LeaderEnd | null }
  | { t: "label-text"; at: P2; leader: LeaderEnd | null };

type Gesture =
  | { t: "pan"; start: P2; orig: { x: number; y: number } }
  | { t: "move"; start: P2; cur: P2; ids: number[]; anchor: P2 | null }
  | { t: "resize"; id: number; handle: Handle; cur: P2 }
  | { t: "box"; start: P2; cur: P2; add: boolean }
  | { t: "slide"; id: number; start: P2; cur: P2 }
  | { t: "standoff"; id: number; cur: P2 }
  | { t: "place-box"; tool: "viewport" | "table" | "image" | "rect"; start: P2; cur: P2 }
  | { t: "place-line"; start: P2; cur: P2 };

type Handle = "nw" | "ne" | "sw" | "se" | "a" | "b";

/** Default sizes of a box item placed with a click (no drag). */
const CLICK_SIZE: Record<"viewport" | "table" | "image" | "rect", P2> = {
  viewport: [0.1016, 0.0762],
  table: [0.127, 0.0635],
  image: [0.0508, 0.0508],
  rect: [0.0254, 0.0254],
};

/** CSS pixels per paper metre at 100% (96 px per inch). */
export const PX_PER_M = 96 / 0.0254;
const HANDLE_PX = 7;

const dist = (a: P2, b: P2) => Math.hypot(a[0] - b[0], a[1] - b[1]);

/** Where an item's grid anchor is (what lands on the grid when it is dragged). */
function anchorOf(k: ItemKind | undefined): P2 | null {
  if (!k) return null;
  const r = itemRect(k);
  if (r) return [r[0], r[1]];
  if (k.kind === "label") return k.at;
  if (k.kind === "line") return k.a;
  return null;
}

function handlesOf(k: ItemKind | undefined): { h: Handle; at: P2 }[] {
  if (!k) return [];
  const r = itemRect(k);
  if (r)
    return [
      { h: "nw", at: [r[0], r[1]] },
      { h: "ne", at: [r[0] + r[2], r[1]] },
      { h: "sw", at: [r[0], r[1] + r[3]] },
      { h: "se", at: [r[0] + r[2], r[1] + r[3]] },
    ];
  if (k.kind === "line")
    return [
      { h: "a", at: k.a },
      { h: "b", at: k.b },
    ];
  return [];
}

/** The rectangle after dragging handle `h` to `p`. */
function resized(r: Rect, h: Handle, p: P2): Rect {
  const fixed: P2 = h === "nw" ? [r[0] + r[2], r[1] + r[3]] : h === "ne" ? [r[0], r[1] + r[3]] : h === "sw" ? [r[0] + r[2], r[1]] : [r[0], r[1]];
  return rectFrom(fixed, p);
}

/** A dimension's paper points (null: its viewport is gone or a point is behind the eye). */
export function dimPaper(r: SheetRender, k: ItemKind): P2[] | null {
  if (k.kind !== "dimension") return null;
  const v = r.viewports.find((x) => x.id === k.viewport);
  if (!v) return null;
  const out: P2[] = [];
  for (const p of k.points) {
    const q = modelToPaper(v, p);
    if (!q) return null;
    out.push(q);
  }
  return out;
}

const SNAP_NAMES: Record<SheetSnap["kind"], string> = { endpoint: "Endpoint", midpoint: "Midpoint", intersection: "Intersection", "on-edge": "On edge" };
const NEEDS: Record<DimKind, number> = { horizontal: 2, vertical: 2, aligned: 2, angular: 3, radius: 3, diameter: 3 };

export const SheetCanvas = forwardRef<SheetCanvasHandle, SheetCanvasProps>(function SheetCanvas(props, ref) {
  const { render, sheet, href, tool, dimKind, selection, gridSnap, onSelection, snap, onMove, onSet, onPlace, onMessage } = props;
  const host = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState<{ w: number; h: number }>({ w: 800, h: 600 });
  // The view: paper origin at (x, y) screen pixels, z pixels per paper metre.
  const [view, setView] = useState<{ x: number; y: number; z: number } | null>(null);
  const viewRef = useRef(view);
  viewRef.current = view;
  const [gesture, setGesture] = useState<Gesture | null>(null);
  const gRef = useRef<Gesture | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [hover, setHover] = useState<P2 | null>(null);
  const [hoverSnap, setHoverSnap] = useState<{ snap: SheetSnap; viewport: number } | null>(null);
  const [text, setText] = useState("");
  const spaceDown = useRef(false);
  const fittedFor = useRef<string>("");

  const setG = (g: Gesture | null) => {
    gRef.current = g;
    setGesture(g);
  };

  const paperW = render?.size[0] ?? 0.2794;
  const paperH = render?.size[1] ?? 0.2159;
  const grid = gridSnap ? (sheet?.grid ?? 0) : 0;
  const items = useMemo(() => new Map((sheet?.items ?? []).map((i) => [i.id, i as ItemKind])), [sheet]);

  const fit = useCallback(() => {
    const pad = 28;
    const z = Math.max(1, Math.min((size.w - 2 * pad) / paperW, (size.h - 2 * pad) / paperH));
    setView({ z, x: (size.w - paperW * z) / 2, y: (size.h - paperH * z) / 2 });
  }, [size, paperW, paperH]);

  // Fit when the canvas first shows a sheet, and when the sheet or its paper changes.
  useLayoutEffect(() => {
    const key = `${render?.id ?? "-"}:${paperW}x${paperH}`;
    if (!render || fittedFor.current === key) return;
    fittedFor.current = key;
    fit();
  }, [render, paperW, paperH, fit]);

  useLayoutEffect(() => {
    const el = host.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const r = el.getBoundingClientRect();
      setSize({ w: Math.max(50, r.width), h: Math.max(50, r.height) });
    });
    ro.observe(el);
    const r = el.getBoundingClientRect();
    setSize({ w: Math.max(50, r.width), h: Math.max(50, r.height) });
    return () => ro.disconnect();
  }, []);
  // Refit on a resize until the person zooms or pans themselves.
  const touched = useRef(false);
  useEffect(() => {
    if (!touched.current && render) fit();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [size]);

  const toPaper = useCallback((cx: number, cy: number): P2 => {
    const v = viewRef.current;
    const r = host.current?.getBoundingClientRect();
    if (!v || !r) return [0, 0];
    return [(cx - r.left - v.x) / v.z, (cy - r.top - v.y) / v.z];
  }, []);
  const toScreen = (p: P2): P2 => (view ? [view.x + p[0] * view.z, view.y + p[1] * view.z] : [0, 0]);
  const px = (n: number) => n / (view?.z ?? 1); // screen pixels in paper metres
  const onGrid = (p: P2): P2 => snapPoint(p, grid);

  // Wheel zoom about the cursor (non-passive, to keep the page from scrolling).
  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const wheel = (e: WheelEvent) => {
      const v = viewRef.current;
      if (!v) return;
      e.preventDefault();
      touched.current = true;
      const r = el.getBoundingClientRect();
      const [sx, sy] = [e.clientX - r.left, e.clientY - r.top];
      const k = Math.exp(-e.deltaY * (e.deltaMode === 1 ? 0.05 : 0.0015));
      const z = Math.min(PX_PER_M * 16, Math.max(PX_PER_M * 0.05, v.z * k));
      const f = z / v.z;
      setView({ z, x: sx - (sx - v.x) * f, y: sy - (sy - v.y) * f });
    };
    el.addEventListener("wheel", wheel, { passive: false });
    return () => el.removeEventListener("wheel", wheel);
  }, []);

  // Space held: drag pans (as in most drawing programs).
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.key === " " && !(e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLSelectElement)) {
        spaceDown.current = true;
        if (host.current?.matches(":hover")) e.preventDefault();
      }
    };
    const up = (e: KeyboardEvent) => {
      if (e.key === " ") spaceDown.current = false;
    };
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
    };
  }, []);

  // A tool change drops what the last one was in the middle of.
  useEffect(() => {
    setPending(null);
    setHoverSnap(null);
    setG(null);
  }, [tool, dimKind, render?.id]);

  useImperativeHandle(ref, () => ({
    cancel() {
      if (gRef.current) {
        setG(null);
        return true;
      }
      if (pending) {
        setPending(null);
        setHoverSnap(null);
        return true;
      }
      return false;
    },
    fit() {
      touched.current = false;
      fit();
    },
    toClient(p: P2) {
      const v = viewRef.current;
      const r = host.current?.getBoundingClientRect();
      if (!v || !r) return null;
      return [r.left + v.x + p[0] * v.z, r.top + v.y + p[1] * v.z];
    },
  }));

  // ---- snapping on a viewport's drawing (dimension and label tools) --------------------
  const snapWant = useRef<{ at: P2; viewport: number } | null>(null);
  const snapBusy = useRef(false);
  const requestSnap = useCallback(
    (at: P2, viewport: number) => {
      snapWant.current = { at, viewport };
      if (snapBusy.current) return;
      const next = () => {
        const w = snapWant.current;
        if (!w) {
          snapBusy.current = false;
          return;
        }
        snapWant.current = null;
        snapBusy.current = true;
        void snap(w.viewport, w.at, 9 / (viewRef.current?.z ?? PX_PER_M))
          .then((s) => setHoverSnap(s ? { snap: s, viewport: w.viewport } : null))
          .catch(() => setHoverSnap(null))
          .finally(next);
      };
      next();
    },
    [snap],
  );

  /** The viewport a dimension or label works on under `p` (the dimension's own once one point is in). */
  const workViewport = (p: P2): ViewportInfo | null => {
    if (!render) return null;
    if (pending?.t === "dim") return render.viewports.find((v) => v.id === pending.viewport) ?? null;
    for (let i = render.viewports.length - 1; i >= 0; i--) {
      const v = render.viewports[i];
      const [x, y, w, h] = v.rect;
      if (p[0] >= x && p[0] <= x + w && p[1] >= y && p[1] <= y + h) return v;
    }
    return null;
  };

  // ---- pointer --------------------------------------------------------------------------
  const onPointerDown = (e: React.PointerEvent) => {
    if (!render || !view) return;
    host.current?.focus();
    const p = toPaper(e.clientX, e.clientY);
    if (e.button === 1 || (e.button === 0 && spaceDown.current)) {
      e.preventDefault();
      (e.currentTarget as Element).setPointerCapture(e.pointerId);
      touched.current = true;
      setG({ t: "pan", start: [e.clientX, e.clientY], orig: { x: view.x, y: view.y } });
      return;
    }
    if (e.button !== 0) return;
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
    if (pending?.t === "label-text") {
      commitLabel();
      return;
    }
    switch (tool) {
      case "select": {
        // A handle of the one selected item.
        if (selection.length === 1) {
          const k = items.get(selection[0]);
          for (const h of handlesOf(k)) {
            if (dist(h.at, p) <= px(HANDLE_PX)) {
              setG({ t: "resize", id: selection[0], handle: h.h, cur: p });
              return;
            }
          }
        }
        const hit = hitItem(render.items, p, px(4));
        if (!hit) {
          setG({ t: "box", start: p, cur: p, add: e.shiftKey });
          if (!e.shiftKey && selection.length) onSelection([]);
          return;
        }
        if (e.shiftKey) {
          onSelection(selection.includes(hit.id) ? selection.filter((x) => x !== hit.id) : [...selection, hit.id]);
          return;
        }
        const ids = selection.includes(hit.id) ? selection : [hit.id];
        if (!selection.includes(hit.id)) onSelection(ids);
        const k = items.get(hit.id);
        if (e.altKey && k?.kind === "viewport") {
          setG({ t: "slide", id: hit.id, start: p, cur: p });
          return;
        }
        if (ids.length === 1 && k?.kind === "dimension") {
          setG({ t: "standoff", id: hit.id, cur: p });
          return;
        }
        setG({ t: "move", start: p, cur: p, ids, anchor: anchorOf(items.get(ids.find((i) => anchorOf(items.get(i))) ?? -1)) });
        return;
      }
      case "viewport":
      case "table":
      case "image":
      case "rect":
        setG({ t: "place-box", tool, start: onGrid(p), cur: onGrid(p) });
        return;
      case "line":
        setG({ t: "place-line", start: onGrid(p), cur: onGrid(p) });
        return;
      case "dimension":
        void dimensionClick(p);
        return;
      case "label":
        void labelClick(p);
        return;
    }
  };

  const onPointerMove = (e: React.PointerEvent) => {
    if (!render || !view) return;
    const p = toPaper(e.clientX, e.clientY);
    setHover(p);
    const g = gRef.current;
    if (g) {
      if (g.t === "pan") {
        setView({ z: view.z, x: g.orig.x + e.clientX - g.start[0], y: g.orig.y + e.clientY - g.start[1] });
        return;
      }
      let cur = p;
      if (g.t === "place-line" && e.shiftKey) {
        // Shift: horizontal or vertical.
        const d: P2 = [p[0] - g.start[0], p[1] - g.start[1]];
        cur = Math.abs(d[0]) >= Math.abs(d[1]) ? [p[0], g.start[1]] : [g.start[0], p[1]];
      }
      if (g.t === "place-box" || g.t === "place-line" || g.t === "resize") cur = onGrid(cur);
      setG({ ...g, cur });
      return;
    }
    if (tool === "dimension" || tool === "label") {
      const v = workViewport(p);
      if (v && (tool === "dimension" || pending === null)) requestSnap(p, v.id);
      else setHoverSnap(null);
    }
  };

  const onPointerUp = (e: React.PointerEvent) => {
    const g = gRef.current;
    if (!g || !render) return;
    setG(null);
    const p = g.t === "pan" ? ([0, 0] as P2) : g.cur;
    switch (g.t) {
      case "move": {
        let by: P2 = [p[0] - g.start[0], p[1] - g.start[1]];
        if (g.anchor && grid > 0) {
          const to = onGrid([g.anchor[0] + by[0], g.anchor[1] + by[1]]);
          by = [to[0] - g.anchor[0], to[1] - g.anchor[1]];
        }
        if (Math.hypot(by[0], by[1]) < 1e-7) return;
        const movable = g.ids.filter((id) => items.get(id)?.kind !== "dimension");
        if (movable.length) onMove(movable, by);
        return;
      }
      case "resize": {
        const k = items.get(g.id);
        if (!k) return;
        const r = itemRect(k);
        if (r) {
          const nr = resized(r, g.handle, p);
          if (nr[2] < 0.002 || nr[3] < 0.002) return onMessage("Too small: an item is at least 2 mm each way.", true);
          if (nr.every((v, i) => Math.abs(v - r[i]) < 1e-9)) return;
          onSet(g.id, { ...k, rect: nr } as ItemKind);
        } else if (k.kind === "line") {
          const next = g.handle === "a" ? { ...k, a: p } : { ...k, b: p };
          if (dist(next.a, next.b) < 1e-4) return;
          onSet(g.id, next);
        }
        return;
      }
      case "box": {
        const r = rectFrom(g.start, p);
        if (r[2] < px(3) && r[3] < px(3)) return;
        const inside = render.items.filter((i) => i.bounds[0] < r[0] + r[2] && i.bounds[0] + i.bounds[2] > r[0] && i.bounds[1] < r[1] + r[3] && i.bounds[1] + i.bounds[3] > r[1]).map((i) => i.id);
        onSelection(g.add ? [...new Set([...selection, ...inside])] : inside);
        return;
      }
      case "slide": {
        const k = items.get(g.id);
        const v = render.viewports.find((x) => x.id === g.id);
        if (!k || k.kind !== "viewport" || !v) return;
        const d: P2 = [p[0] - g.start[0], p[1] - g.start[1]];
        if (Math.hypot(d[0], d[1]) < 1e-6) return;
        onSet(g.id, { ...k, centre: [v.centre[0] - d[0] / v.factor, v.centre[1] + d[1] / v.factor] });
        return;
      }
      case "standoff": {
        const k = items.get(g.id);
        if (!k || k.kind !== "dimension") return;
        const pts = dimPaper(render, k);
        if (!pts || dist(p, pts[0]) < 1e-9) return;
        const offset = dimOffset(k.dim, pts, p);
        if (Math.abs(offset - k.offset) < 1e-6) return;
        onSet(g.id, { ...k, offset });
        return;
      }
      case "place-box": {
        const r = rectFrom(g.start, p);
        const dragged = r[2] >= 0.002 && r[3] >= 0.002;
        const s = CLICK_SIZE[g.tool];
        onPlace({ kind: "box", tool: g.tool, rect: dragged ? r : [g.start[0], g.start[1], s[0], s[1]], dragged });
        return;
      }
      case "place-line": {
        if (dist(g.start, p) < 0.001) return onMessage("Drag to draw a line.", false);
        onPlace({ kind: "line", a: g.start, b: p });
        return;
      }
      case "pan":
        void e;
        return;
    }
  };

  // ---- the dimension tool: points on one viewport's drawing, then where the line stands --
  const snapAt = async (p: P2, viewport: number): Promise<SheetSnap | null> => {
    const s = hoverSnap;
    if (s && s.viewport === viewport && dist(s.snap.at, p) <= px(9)) return s.snap;
    return snap(viewport, p, px(9));
  };

  const dimensionClick = async (p: P2) => {
    if (!render) return;
    const need = NEEDS[dimKind];
    if (pending?.t === "dim" && pending.points.length >= need) {
      const offset = dimOffset(dimKind, pending.paper, p);
      onPlace({ kind: "dimension", viewport: pending.viewport, dim: dimKind, points: pending.points, offset });
      setPending(null);
      return;
    }
    const v = workViewport(p);
    if (!v) return onMessage("Click a point of the drawing in a viewport: an endpoint, a midpoint, an intersection or a point on an edge.", true);
    const s = await snapAt(p, v.id);
    if (!s) return onMessage("Nothing to dimension there: click on a line of the drawing.", true);
    const prev = pending?.t === "dim" ? pending : { t: "dim" as const, viewport: v.id, points: [] as V3[], paper: [] as P2[] };
    const next = { ...prev, points: [...prev.points, s.point], paper: [...prev.paper, s.at] };
    setPending(next);
    const left = need - next.points.length;
    onMessage(left > 0 ? `${SNAP_NAMES[s.kind]}. ${left === 1 ? "One more point" : `${left} more points`}${dimKind === "angular" && next.points.length === 1 ? " (the first was the corner)" : ""}.` : "Now click where the dimension line goes.", false);
  };

  // ---- the label tool: what it points at, where its text goes, then the words ------------
  const labelClick = async (p: P2) => {
    if (!render) return;
    if (pending?.t === "label-tip") {
      const at = onGrid(p);
      const near = dist(at, pending.tip) < 0.002;
      setPending({ t: "label-text", at: near ? pending.tip : at, leader: near ? null : pending.leader });
      setText("");
      return;
    }
    const v = workViewport(p);
    let leader: LeaderEnd | null = null;
    let tip = onGrid(p);
    if (v) {
      const s = await snapAt(p, v.id);
      if (s) {
        leader = { on: "model", viewport: v.id, point: s.point };
        tip = s.at;
      }
    }
    if (!leader) leader = { on: "paper", at: tip };
    setPending({ t: "label-tip", tip, leader });
    onMessage("Now click where the text goes (click the same spot for no leader).", false);
  };

  const commitLabel = () => {
    if (pending?.t !== "label-text") return;
    const t = text.replace(/\s+$/, "");
    setPending(null);
    setText("");
    if (!t.trim()) return;
    onPlace({ kind: "label", at: pending.at, leader: pending.leader, text: t });
  };

  // ---- drawing ----------------------------------------------------------------------------
  const layers = useMemo(() => (render ? sheetLayers(render, href, CANVAS_K) : null), [render, href]);
  const moving = gesture?.t === "move" ? gesture : null;
  const moveBy = useMemo<P2>(() => {
    if (!moving) return [0, 0];
    let by: P2 = [moving.cur[0] - moving.start[0], moving.cur[1] - moving.start[1]];
    if (moving.anchor && grid > 0) {
      const to = snapPoint([moving.anchor[0] + by[0], moving.anchor[1] + by[1]], grid);
      by = [to[0] - moving.anchor[0], to[1] - moving.anchor[1]];
    }
    return by;
  }, [moving, grid]);
  const movingSet = useMemo(() => {
    if (!moving) return new Set<number>();
    const ids = new Set(moving.ids.filter((id) => items.get(id)?.kind !== "dimension"));
    // Dimensions ride along with their viewport.
    for (const [id, k] of items) if (k.kind === "dimension" && ids.has(k.viewport)) ids.add(id);
    return ids;
  }, [moving, items]);

  const K = CANVAS_K;
  const paperT = view ? `translate(${view.x} ${view.y}) scale(${view.z / K})` : undefined;
  const gridPx = grid > 0 && view ? grid * view.z : 0;
  const info = (id: number): ItemInfo | undefined => render?.items.find((i) => i.id === id);

  // Overlay (screen pixels).
  const overlay: React.ReactNode[] = [];
  if (render && view) {
    const box = (r: Rect, cls: string, key: string) => {
      const [a, b] = [toScreen([r[0], r[1]]), toScreen([r[0] + r[2], r[1] + r[3]])];
      return <rect key={key} className={cls} x={a[0]} y={a[1]} width={Math.max(1, b[0] - a[0])} height={Math.max(1, b[1] - a[1])} />;
    };
    if (tool === "select" && hover && !gesture) {
      const h = hitItem(render.items, hover, px(4));
      if (h && !selection.includes(h.id)) overlay.push(box(h.bounds, "sh-hover", `hover-${h.id}`));
    }
    for (const id of selection) {
      const it = info(id);
      if (!it) continue;
      const b = it.bounds;
      const d = movingSet.has(id) ? moveBy : [0, 0];
      overlay.push(box([b[0] + d[0], b[1] + d[1], b[2], b[3]], "sh-sel", `sel-${id}`));
    }
    if (selection.length === 1 && !moving) {
      for (const h of handlesOf(items.get(selection[0]))) {
        const s = toScreen(h.at);
        overlay.push(<rect key={`h-${h.h}`} className="sh-handle" data-handle={h.h} x={s[0] - HANDLE_PX / 2} y={s[1] - HANDLE_PX / 2} width={HANDLE_PX} height={HANDLE_PX} />);
      }
    }
    const g = gesture;
    if (g?.t === "box") overlay.push(box(rectFrom(g.start, g.cur), "sh-box", "box"));
    if (g?.t === "place-box") overlay.push(box(rectFrom(g.start, g.cur), "sh-place", "place"));
    if (g?.t === "resize") {
      const k = items.get(g.id);
      const r = k && itemRect(k);
      if (r) overlay.push(box(resized(r, g.handle, g.cur), "sh-place", "resize"));
      else if (k?.kind === "line") {
        const [a, b] = [toScreen(g.handle === "a" ? g.cur : k.a), toScreen(g.handle === "b" ? g.cur : k.b)];
        overlay.push(<line key="resize" className="sh-place" x1={a[0]} y1={a[1]} x2={b[0]} y2={b[1]} />);
      }
    }
    if (g?.t === "place-line") {
      const [a, b] = [toScreen(g.start), toScreen(g.cur)];
      overlay.push(<line key="pl" className="sh-place" x1={a[0]} y1={a[1]} x2={b[0]} y2={b[1]} />);
    }
    if (g?.t === "slide") {
      const v = render.viewports.find((x) => x.id === g.id);
      if (v) {
        const [a, b] = [toScreen(g.start), toScreen(g.cur)];
        overlay.push(<line key="slide" className="sh-place" x1={a[0]} y1={a[1]} x2={b[0]} y2={b[1]} />);
        overlay.push(box(v.rect, "sh-place", "slide-r"));
      }
    }
    if (g?.t === "standoff") {
      const k = items.get(g.id);
      const pts = k && dimPaper(render, k);
      if (k?.kind === "dimension" && pts) {
        const line = dimPreview(k.dim, pts, dimOffset(k.dim, pts, g.cur));
        if (line) {
          const [a, b] = [toScreen(line[0]), toScreen(line[1])];
          overlay.push(<line key="so" className="sh-place" x1={a[0]} y1={a[1]} x2={b[0]} y2={b[1]} />);
        }
      }
    }
    if (pending?.t === "dim") {
      pending.paper.forEach((q, i) => {
        const s = toScreen(q);
        overlay.push(<circle key={`dp-${i}`} className="sh-picked" cx={s[0]} cy={s[1]} r={4} />);
      });
      if (pending.points.length >= NEEDS[dimKind] && hover) {
        const off = dimOffset(dimKind, pending.paper, hover);
        const line = dimPreview(dimKind, pending.paper, off);
        const ends: P2[] = line ?? [pending.paper[0], hover];
        const [a, b] = [toScreen(ends[0]), toScreen(ends[1])];
        overlay.push(<line key="dprev" className="sh-place" x1={a[0]} y1={a[1]} x2={b[0]} y2={b[1]} />);
        if (line) {
          for (const [i, q] of pending.paper.slice(0, 2).entries()) {
            const [c, d] = [toScreen(q), toScreen(line[i])];
            overlay.push(<line key={`dext-${i}`} className="sh-place thin" x1={c[0]} y1={c[1]} x2={d[0]} y2={d[1]} />);
          }
        }
      }
    }
    if (pending?.t === "label-tip" && hover) {
      const [a, b] = [toScreen(pending.tip), toScreen(onGrid(hover))];
      overlay.push(<line key="lt" className="sh-place" x1={a[0]} y1={a[1]} x2={b[0]} y2={b[1]} />);
      overlay.push(<circle key="ltc" className="sh-picked" cx={a[0]} cy={a[1]} r={3.5} />);
    }
    if (hoverSnap && (tool === "dimension" || tool === "label") && !(pending?.t === "dim" && pending.points.length >= NEEDS[dimKind])) {
      const s = toScreen(hoverSnap.snap.at);
      overlay.push(
        <g key="snap" className={`sh-snap ${hoverSnap.snap.kind}`} data-snap={hoverSnap.snap.kind}>
          {hoverSnap.snap.kind === "endpoint" ? <rect x={s[0] - 5} y={s[1] - 5} width={10} height={10} /> : hoverSnap.snap.kind === "midpoint" ? <path d={`M${s[0]} ${s[1] - 6}L${s[0] + 6} ${s[1] + 5}L${s[0] - 6} ${s[1] + 5}Z`} /> : hoverSnap.snap.kind === "intersection" ? <path d={`M${s[0] - 5} ${s[1] - 5}L${s[0] + 5} ${s[1] + 5}M${s[0] + 5} ${s[1] - 5}L${s[0] - 5} ${s[1] + 5}`} /> : <circle cx={s[0]} cy={s[1]} r={4.5} />}
          <text x={s[0] + 9} y={s[1] - 8}>
            {SNAP_NAMES[hoverSnap.snap.kind]}
          </text>
        </g>,
      );
    }
  }

  const cursor = gesture?.t === "pan" || spaceDown.current ? "grabbing" : tool === "select" ? (hover && render && hitItem(render.items, hover, px(4)) ? "move" : "default") : "crosshair";
  const textAt = pending?.t === "label-text" && view ? toScreen(pending.at) : null;

  return (
    <div
      ref={host}
      className="sheet-canvas"
      tabIndex={0}
      role="application"
      aria-label={render ? `Sheet ${render.name}` : "Sheet"}
      data-tool={tool}
      style={{ cursor }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerLeave={() => {
        setHover(null);
        if (!gRef.current) setHoverSnap(null);
      }}
      onContextMenu={(e) => e.preventDefault()}
    >
      <svg className="sheet-svg" width={size.w} height={size.h} data-testid="sheet-svg">
        {render && view ? (
          <>
            <defs>
              {gridPx >= 7 ? (
                <pattern id="sheet-grid" patternUnits="userSpaceOnUse" width={gridPx} height={gridPx} x={view.x} y={view.y}>
                  <circle cx={0.5} cy={0.5} r={0.75} className="sheet-grid-dot" />
                </pattern>
              ) : null}
            </defs>
            <rect className="sheet-shadow" x={view.x + 3} y={view.y + 4} width={paperW * view.z} height={paperH * view.z} />
            <g transform={paperT}>
              <g className="sheet-layer" dangerouslySetInnerHTML={{ __html: layers ? `<rect x="0" y="0" width="${paperW * K}" height="${paperH * K}" fill="#ffffff"/>${layers.under}` : "" }} />
            </g>
            {gridPx >= 7 ? <rect className="sheet-grid" x={view.x} y={view.y} width={paperW * view.z} height={paperH * view.z} fill="url(#sheet-grid)" /> : null}
            <g transform={paperT}>
              {layers?.items.map((it) => (
                <g key={it.id} data-item={it.id} transform={movingSet.has(it.id) ? `translate(${moveBy[0] * K} ${moveBy[1] * K})` : undefined} dangerouslySetInnerHTML={{ __html: it.svg }} />
              ))}
              <g dangerouslySetInnerHTML={{ __html: layers?.over ?? "" }} />
            </g>
            <g className="sheet-overlay">{overlay}</g>
          </>
        ) : null}
      </svg>
      {textAt ? (
        <textarea
          className="sheet-text"
          aria-label="Label text"
          autoFocus
          rows={2}
          style={{ left: textAt[0], top: textAt[1] - 14 }}
          value={text}
          placeholder="Label text"
          onPointerDown={(e) => e.stopPropagation()}
          onChange={(e) => setText(e.currentTarget.value)}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              commitLabel();
            } else if (e.key === "Escape") {
              setPending(null);
              setText("");
            }
          }}
        />
      ) : null}
    </div>
  );
});
