import { useEffect, useRef, useState } from "react";
import type { MeshData } from "../kernel/types";
import { INFERENCE_STYLES, type HoverHit } from "../lib/hoverInference";
import { ViewportEngine, webgl2Available, type ViewId } from "../viewport/engine";

export type KernelState = "loading" | "ready" | "missing" | "error";

export interface ViewportProps {
  mesh: MeshData | null;
  view: ViewId;
  /** HUD chips, top-left, as in the mock. */
  hud: { text: string; tone?: "ok" | "err" }[];
  kernel: KernelState;
}

const LEGEND = (["endpoint", "midpoint", "on-edge", "on-face"] as const).map((k) => INFERENCE_STYLES[k]);

export function Viewport({ mesh, view, hud, kernel }: ViewportProps) {
  const hostRef = useRef<HTMLElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const engineRef = useRef<ViewportEngine | null>(null);
  const [hover, setHover] = useState<HoverHit | null>(null);
  const [glError, setGlError] = useState<string | null>(null);

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
    // Read-only hook for e2e tests and lane-end captures (scripts/capture.mjs).
    (window as unknown as { __sawhorse?: object }).__sawhorse = { visibleEndpoints: () => engine.visibleEndpoints() };
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
      engine.dispose();
      engineRef.current = null;
    };
  }, []);

  useEffect(() => {
    engineRef.current?.setMesh(mesh);
    setHover(null);
  }, [mesh]);

  useEffect(() => {
    engineRef.current?.setView(view);
  }, [view]);

  // Coarse hover inference, at most once per frame.
  const pending = useRef<number | null>(null);
  const onPointerMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    if (pending.current !== null) cancelAnimationFrame(pending.current);
    pending.current = requestAnimationFrame(() => {
      pending.current = null;
      setHover(engineRef.current?.hover(x, y) ?? null);
    });
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
      data-watertight={mesh ? String(mesh.stats.watertight) : undefined}
      data-hash={mesh?.stats.hash}
    >
      <canvas
        id="cv"
        ref={canvasRef}
        aria-label="3D viewport"
        onPointerMove={onPointerMove}
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
