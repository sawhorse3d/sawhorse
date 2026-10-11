import { useEffect, useRef, useState } from "react";
import type { MeshData, MeshPart } from "../kernel/types";
import type { Vec3 } from "../lib/edit";
import { INFERENCE_STYLES, type HoverHit } from "../lib/hoverInference";
import { ViewportEngine, webgl2Available, type ViewId } from "../viewport/engine";

export type KernelState = "loading" | "ready" | "missing" | "error";

export interface ViewportProps {
  mesh: MeshData | null;
  view: ViewId;
  /** HUD chips, top-left, as in the mock. */
  hud: { text: string; tone?: "ok" | "err" }[];
  kernel: KernelState;
  /** Data attributes for tests and captures (state hash, watertight, objects, undo). */
  info?: { hash?: string; watertight?: boolean; objects?: number; canUndo?: boolean; canRedo?: boolean };
  /** Highlighted part (index into mesh.parts), -1 for none. */
  selection: number;
  /** Outline being drawn (e.g. the rectangle under the cursor). */
  preview: Vec3[] | null;
  /** A left click (not a drag) at canvas CSS pixel (x, y). */
  onCanvasClick?: (x: number, y: number) => void;
  /** Pointer moved over the canvas at (x, y). */
  onCanvasMove?: (x: number, y: number) => void;
  /** The engine, once it exists (null when it goes away). */
  onEngine?: (engine: ViewportEngine | null) => void;
}

const LEGEND = (["endpoint", "midpoint", "on-edge", "on-face"] as const).map((k) => INFERENCE_STYLES[k]);

/** Read-only hooks for e2e tests and lane-end captures (scripts/capture*.mjs). */
interface SawhorseHooks {
  visibleEndpoints(): [number, number][];
  parts(): MeshPart[];
  partScreenCenter(part: number): [number, number] | null;
  worldToScreen(p: Vec3): [number, number];
}

export function Viewport({ mesh, view, hud, kernel, info, selection, preview, onCanvasClick, onCanvasMove, onEngine }: ViewportProps) {
  const hostRef = useRef<HTMLElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const engineRef = useRef<ViewportEngine | null>(null);
  const meshRef = useRef<MeshData | null>(mesh);
  meshRef.current = mesh;
  const [hover, setHover] = useState<HoverHit | null>(null);
  const [glError, setGlError] = useState<string | null>(null);
  const onEngineRef = useRef(onEngine);
  onEngineRef.current = onEngine;

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
    onEngineRef.current?.(engine);
    const hooks: SawhorseHooks = {
      visibleEndpoints: () => engine.visibleEndpoints(),
      parts: () => meshRef.current?.parts ?? [],
      partScreenCenter: (part) => engine.partScreenCenter(part),
      worldToScreen: (p) => engine.worldToScreen(p),
    };
    (window as unknown as { __sawhorse?: SawhorseHooks }).__sawhorse = hooks;
    const ro = new ResizeObserver(() => {
      const r = host.getBoundingClientRect();
      engine.resize(r.width, r.height);
    });
    ro.observe(host);
    const r = host.getBoundingClientRect();
    engine.resize(r.width, r.height);

    // Theme: OS preference and the data-theme override both repaint the scene.
    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    const onTheme = () => engine.applyTheme();
    mq?.addEventListener?.("change", onTheme);
    const mo = new MutationObserver(onTheme);
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });

    return () => {
      ro.disconnect();
      mo.disconnect();
      mq?.removeEventListener?.("change", onTheme);
      onEngineRef.current?.(null);
      engine.dispose();
      engineRef.current = null;
    };
  }, []);

  useEffect(() => {
    engineRef.current?.setMesh(mesh);
    setHover(null);
  }, [mesh]);

  useEffect(() => {
    engineRef.current?.setSelection(selection);
  }, [selection, mesh]);

  useEffect(() => {
    engineRef.current?.setPreview(preview);
  }, [preview]);

  useEffect(() => {
    engineRef.current?.setView(view);
  }, [view]);

  // Coarse hover inference, at most once per frame.
  const pending = useRef<number | null>(null);
  const local = (e: React.PointerEvent<HTMLCanvasElement>): [number, number] => {
    const rect = e.currentTarget.getBoundingClientRect();
    return [e.clientX - rect.left, e.clientY - rect.top];
  };
  const onPointerMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const [x, y] = local(e);
    onCanvasMove?.(x, y);
    if (pending.current !== null) cancelAnimationFrame(pending.current);
    pending.current = requestAnimationFrame(() => {
      pending.current = null;
      setHover(engineRef.current?.hover(x, y) ?? null);
    });
  };
  // A click is a left press and release that barely moved (the left button belongs to the tool).
  const down = useRef<[number, number] | null>(null);
  const onPointerDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    down.current = e.button === 0 ? local(e) : null;
  };
  const onPointerUp = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const start = down.current;
    down.current = null;
    if (!start || e.button !== 0) return;
    const [x, y] = local(e);
    if (Math.hypot(x - start[0], y - start[1]) <= 4) onCanvasClick?.(x, y);
  };

  const tri = mesh ? mesh.indices.length / 3 : 0;
  const style = hover ? INFERENCE_STYLES[hover.kind] : null;

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
      data-can-undo={info?.canUndo === undefined ? undefined : String(info.canUndo)}
      data-can-redo={info?.canRedo === undefined ? undefined : String(info.canRedo)}
      data-selection={selection}
    >
      <canvas
        id="cv"
        ref={canvasRef}
        aria-label="3D viewport"
        onPointerMove={onPointerMove}
        onPointerDown={onPointerDown}
        onPointerUp={onPointerUp}
        onPointerLeave={() => setHover(null)}
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
      {hover && style ? (
        <>
          <span
            className="snap"
            aria-hidden="true"
            style={{ left: hover.at[0], top: hover.at[1], background: `var(${style.token})` }}
          ></span>
          <div className="tip" id="tip" style={{ left: hover.at[0] + 12, top: hover.at[1] - 26 }}>
            <i style={{ background: `var(${style.token})` }}></i>
            {style.label}
          </div>
        </>
      ) : null}
      <div className="legend">
        {LEGEND.map((s) => (
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
