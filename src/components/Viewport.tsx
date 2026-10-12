import { useEffect, useRef, useState } from "react";
import type { MeshData, MeshPart } from "../kernel/types";
import type { Vec3 } from "../lib/edit";
import { INFERENCE_STYLES, LEGEND_KINDS, type InferenceKind } from "../lib/inferenceStyle";
import type { UnitSystem } from "../lib/units";
import { NO_HIGHLIGHT, ViewportEngine, webgl2Available, type Highlight, type OverlayItem, type Projection, type ViewId } from "../viewport/engine";

export type KernelState = "loading" | "ready" | "missing" | "error";

/** A pointer event on the canvas, in canvas CSS pixels, with the modifier keys. */
export interface ViewportPointer {
  type: "down" | "move" | "up" | "dblclick" | "leave";
  x: number;
  y: number;
  /** 0 left, 1 middle, 2 right. */
  button: number;
  shift: boolean;
  ctrl: boolean;
  alt: boolean;
}

/** The inference marker: a dot in the inference's colour plus its tooltip. */
export interface Marker {
  at: Vec3;
  kind: InferenceKind;
  tip: string;
}

/** A text label pinned to a world point (dimension text, tape readout). */
export interface Label {
  at: Vec3;
  text: string;
  className?: string;
}

/** Window (left to right, solid) or crossing (right to left, dashed) selection rectangle. */
export interface SelectionBox {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
  crossing: boolean;
}

export interface ViewportProps {
  mesh: MeshData | null;
  units: UnitSystem;
  /** Requested standard view; `viewSeq` changes re-apply it even when the id repeats. */
  view: ViewId;
  viewSeq: number;
  projection: Projection;
  /** Bumped to request a zoom extents. */
  fitSeq: number;
  /** HUD chips, top-left, as in the mock. */
  hud: { text: string; tone?: "ok" | "err" }[];
  kernel: KernelState;
  /** Data attributes for tests and captures (state hash, watertight, objects, undo). */
  info?: { hash?: string; watertight?: boolean; objects?: number; canUndo?: boolean; canRedo?: boolean; volume?: number; solids?: number };
  highlight?: Highlight;
  overlay?: OverlayItem[];
  marker?: Marker | null;
  labels?: Label[];
  box?: SelectionBox | null;
  cursor?: string;
  /** Number of selected entities (a data attribute for tests). */
  selectionCount?: number;
  /** Kind of the entity under the cursor (a data attribute for tests). */
  hoverKind?: string;
  onPointer?: (p: ViewportPointer) => void;
  /** The camera left the standard view (orbit) or changed projection. */
  onCamera?: (view: ViewId | null, projection: Projection) => void;
  /** The engine, once it exists (null when it goes away). */
  onEngine?: (engine: ViewportEngine | null) => void;
}

/** Read-only hooks for e2e tests and lane-end captures (scripts/capture*.mjs, e2e/*.spec.ts). */
export interface SawhorseHooks {
  visibleEndpoints(): [number, number][];
  parts(): MeshPart[];
  partScreenCenter(part: number): [number, number] | null;
  worldToScreen(p: Vec3): [number, number];
  camera(): ReturnType<ViewportEngine["cameraSummary"]>;
  grid(): ReturnType<ViewportEngine["gridState"]>;
  /** Coarse pick under canvas pixel (x, y): world point and part index (-1 = ground). */
  pick(x: number, y: number): { point: Vec3; part: number } | null;
  /** Read back canvas pixels (r, g, b) at canvas pixel (x, y). */
  pixel(x: number, y: number): [number, number, number];
  /** Milliseconds per frame over `n` renders with the camera turning (lane 10). */
  frameTime(n?: number): number;
  /** Triangles of the mesh the engine draws (what the render passes read). */
  engineTriangles(): number;
}

export function Viewport(props: ViewportProps) {
  const { mesh, units, view, viewSeq, projection, fitSeq, hud, kernel, info, highlight, overlay, marker, labels, box, cursor, selectionCount, hoverKind, onPointer, onCamera, onEngine } = props;
  const hostRef = useRef<HTMLElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const engineRef = useRef<ViewportEngine | null>(null);
  const meshRef = useRef<MeshData | null>(mesh);
  meshRef.current = mesh;
  const [glError, setGlError] = useState<string | null>(null);
  const [frame, setFrame] = useState(0);
  const cb = useRef({ onEngine, onCamera, onPointer });
  cb.current = { onEngine, onCamera, onPointer };
  const [camAttr, setCamAttr] = useState<{ view: ViewId | null; projection: Projection; eye: string }>({ view: "iso", projection: "parallel", eye: "" });

  // Engine lifetime.
  useEffect(() => {
    const canvas = canvasRef.current;
    const host = hostRef.current;
    if (!canvas || !host) return;
    if (!webgl2Available()) {
      setGlError("WebGL2 is not available in this browser.");
      return;
    }
    let engine: ViewportEngine;
    try {
      engine = new ViewportEngine(canvas);
    } catch (e) {
      setGlError(e instanceof Error ? e.message : String(e));
      return;
    }
    engineRef.current = engine;
    cb.current.onEngine?.(engine);
    const hooks: SawhorseHooks = {
      visibleEndpoints: () => engine.visibleEndpoints(),
      parts: () => meshRef.current?.parts ?? [],
      partScreenCenter: (part) => engine.partScreenCenter(part),
      worldToScreen: (p) => engine.worldToScreen(p),
      camera: () => engine.cameraSummary(),
      grid: () => engine.gridState(),
      pick: (x, y) => engine.pick(x, y),
      pixel: (x, y) => engine.readPixel(x, y),
      frameTime: (n) => engine.benchmark(n),
      engineTriangles: () => (engine.captureSource().mesh?.indices.length ?? 0) / 3,
    };
    const w = window as unknown as { __sawhorse?: Record<string, unknown> };
    w.__sawhorse = { ...(w.__sawhorse ?? {}), ...hooks };
    const ro = new ResizeObserver(() => {
      const r = host.getBoundingClientRect();
      engine.resize(r.width, r.height);
    });
    ro.observe(host);
    const r = host.getBoundingClientRect();
    engine.resize(r.width, r.height);
    let last = "";
    const off = engine.onRender(() => {
      // Labels and the marker follow the camera; the camera summary feeds tests.
      setFrame((f) => (f + 1) % 1_000_000);
      const s = engine.cameraSummary();
      const key = `${s.view}|${s.projection}|${s.eye.join(",")}|${s.zoom}`;
      if (key !== last) {
        last = key;
        setCamAttr({ view: s.view, projection: s.projection, eye: `${s.eye.join(",")}@${s.zoom}` });
        cb.current.onCamera?.(s.view, s.projection);
      }
    });

    // Theme: OS preference and the data-theme override both repaint the scene.
    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    const onTheme = () => engine.applyTheme();
    mq?.addEventListener?.("change", onTheme);
    const mo = new MutationObserver(onTheme);
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });

    return () => {
      off();
      ro.disconnect();
      mo.disconnect();
      mq?.removeEventListener?.("change", onTheme);
      cb.current.onEngine?.(null);
      engine.dispose();
      engineRef.current = null;
    };
  }, []);

  useEffect(() => {
    engineRef.current?.setMesh(mesh);
  }, [mesh]);

  useEffect(() => {
    engineRef.current?.setUnits(units);
  }, [units]);

  useEffect(() => {
    engineRef.current?.setView(view);
  }, [view, viewSeq]);

  useEffect(() => {
    engineRef.current?.setProjection(projection);
  }, [projection]);

  useEffect(() => {
    if (fitSeq > 0) engineRef.current?.fit();
  }, [fitSeq]);

  useEffect(() => {
    engineRef.current?.setHighlight(highlight ?? NO_HIGHLIGHT);
  }, [highlight, mesh]);

  useEffect(() => {
    engineRef.current?.setOverlay(overlay ?? []);
  }, [overlay]);

  const local = (e: React.PointerEvent<HTMLCanvasElement> | React.MouseEvent<HTMLCanvasElement>): [number, number] => {
    const rect = e.currentTarget.getBoundingClientRect();
    return [e.clientX - rect.left, e.clientY - rect.top];
  };
  const send = (type: ViewportPointer["type"], e: React.PointerEvent<HTMLCanvasElement> | React.MouseEvent<HTMLCanvasElement>) => {
    const [x, y] = local(e);
    cb.current.onPointer?.({ type, x, y, button: e.button, shift: e.shiftKey, ctrl: e.ctrlKey || e.metaKey, alt: e.altKey });
  };

  const tri = mesh ? mesh.indices.length / 3 : 0;
  const engine = engineRef.current;
  void frame; // re-render with the camera so pinned labels follow it
  const markerXY = marker && engine ? engine.worldToScreen(marker.at) : null;
  const style = marker ? INFERENCE_STYLES[marker.kind] : null;

  return (
    <main
      className="vp"
      aria-label="Viewport"
      ref={hostRef}
      data-kernel={kernel}
      data-triangles={tri}
      data-parts={mesh?.parts.length ?? 0}
      data-watertight={info?.watertight === undefined ? undefined : String(info.watertight)}
      data-hash={info?.hash}
      data-objects={info?.objects}
      data-volume={info?.volume === undefined ? undefined : String(info.volume)}
      data-can-undo={info?.canUndo === undefined ? undefined : String(info.canUndo)}
      data-can-redo={info?.canRedo === undefined ? undefined : String(info.canRedo)}
      data-view={camAttr.view ?? "custom"}
      data-projection={camAttr.projection}
      data-camera={camAttr.eye}
      data-inference={marker?.kind ?? ""}
      data-selection={selectionCount ?? 0}
      data-hover={hoverKind ?? ""}
    >
      <canvas
        id="cv"
        ref={canvasRef}
        aria-label="3D viewport"
        style={cursor ? { cursor } : undefined}
        onPointerMove={(e) => send("move", e)}
        onPointerDown={(e) => {
          if (e.button === 1) e.preventDefault(); // no autoscroll on middle press
          send("down", e);
        }}
        onPointerUp={(e) => send("up", e)}
        onDoubleClick={(e) => send("dblclick", e)}
        onPointerLeave={(e) => send("leave", e)}
        onContextMenu={(e) => e.preventDefault()}
      ></canvas>
      <div className="hud">
        {glError ? <span className="err">{glError}</span> : null}
        {hud.map((h, i) => (
          <span key={i} className={h.tone}>
            {h.text}
          </span>
        ))}
      </div>
      {engine
        ? (labels ?? []).map((l, i) => {
            const [x, y] = engine.worldToScreen(l.at);
            return (
              <div key={i} className={`vlabel ${l.className ?? ""}`} style={{ left: x, top: y }}>
                {l.text}
              </div>
            );
          })
        : null}
      {box ? (
        <div
          className={box.crossing ? "selbox crossing" : "selbox"}
          style={{ left: Math.min(box.x0, box.x1), top: Math.min(box.y0, box.y1), width: Math.abs(box.x1 - box.x0), height: Math.abs(box.y1 - box.y0) }}
        ></div>
      ) : null}
      {marker && markerXY && style ? (
        <>
          <span className="snap" aria-hidden="true" style={{ left: markerXY[0], top: markerXY[1], background: `var(${style.token})` }}></span>
          <div className="tip" id="tip" style={{ left: markerXY[0] + 12, top: markerXY[1] - 26 }}>
            <i style={{ background: `var(${style.token})` }}></i>
            {marker.tip}
          </div>
        </>
      ) : null}
      <div className="legend">
        {LEGEND_KINDS.map((k) => INFERENCE_STYLES[k]).map((s) => (
          <span key={s.token}>
            <i style={{ background: `var(${s.token})` }}></i>
            {s.label}
          </span>
        ))}
      </div>
      <div className="axes">x ─ red · y ─ green · z ─ blue</div>
    </main>
  );
}
