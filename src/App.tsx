import { useCallback, useEffect, useRef, useState } from "react";
import { TopBar } from "./components/TopBar";
import { ToolRail } from "./components/ToolRail";
import { Viewport, type KernelState } from "./components/Viewport";
import { SidePanel } from "./components/SidePanel";
import { ValueBar } from "./components/ValueBar";
import { createKernel, KernelUnavailableError, type EditResult, type KernelClient } from "./kernel/client";
import type { KernelOp, SceneData } from "./kernel/types";
import {
  GROUND,
  describeTarget,
  frameOfPart,
  nextTarget,
  partIndex,
  pushOp,
  rayPlane,
  rectCorners,
  rectangleOp,
  targetOfPart,
  toLocal,
  toWorld,
  type RectAnchor,
  type Target,
  type Vec3,
} from "./lib/edit";
import { startsValueEntry, toolById, toolForKey, type ToolId } from "./lib/tools";
import {
  formatLength,
  M_PER_IN,
  nextUnitSystem,
  parseDimensions,
  parseLength,
  unitInfo,
  UNIT_SYSTEMS,
  type UnitSystem,
} from "./lib/units";
import type { ViewId, ViewportEngine } from "./viewport/engine";

/** What the Value Control Box offers before anything is typed: 12 in up, 24 x 16 in rectangles. */
export const DEFAULT_DISTANCE = 12 * M_PER_IN;
export const DEFAULT_RECT: [number, number] = [24 * M_PER_IN, 16 * M_PER_IN];

const UNITS_KEY = "sawhorse.units";

function loadUnits(): UnitSystem {
  try {
    const v = localStorage.getItem(UNITS_KEY);
    if (v && UNIT_SYSTEMS.some((u) => u.id === v)) return v as UnitSystem;
  } catch {
    /* storage blocked: fall back to the default */
  }
  return "frac-in";
}

function saveUnits(u: UnitSystem): void {
  try {
    localStorage.setItem(UNITS_KEY, u);
  } catch {
    /* per-viewer convenience only */
  }
}

function isTextEntry(el: EventTarget | null): boolean {
  if (!(el instanceof HTMLElement)) return false;
  return el.isContentEditable || el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement;
}

type Message = { text: string; error: boolean } | null;

export function App() {
  const [toolId, setToolId] = useState<ToolId>("select");
  const tool = toolById(toolId);
  const [view, setView] = useState<ViewId>("iso");
  const [units, setUnits] = useState<UnitSystem>(loadUnits);
  const [scene, setScene] = useState<SceneData | null>(null);
  const [kernelState, setKernelState] = useState<KernelState>("loading");
  const [kernelNote, setKernelNote] = useState<string | null>(null);
  const [vcb, setVcb] = useState("");
  const [message, setMessage] = useState<Message>(null);
  const [target, setTarget] = useState<Target | null>(null);
  const [rectStart, setRectStart] = useState<RectAnchor | null>(null);
  const [preview, setPreview] = useState<Vec3[] | null>(null);
  const [lastDistance, setLastDistance] = useState(DEFAULT_DISTANCE);
  const [lastRect, setLastRect] = useState<[number, number]>(DEFAULT_RECT);
  const vcbRef = useRef<HTMLInputElement>(null);
  const kernelRef = useRef<KernelClient | null>(null);
  const engineRef = useRef<ViewportEngine | null>(null);
  const targetRef = useRef<Target | null>(target);
  targetRef.current = target;
  // Edits run one at a time, in order; a click during an edit waits its turn.
  const queue = useRef<Promise<unknown>>(Promise.resolve());

  const vcbValueFor = useCallback(
    (id: ToolId, u: UnitSystem): string => {
      switch (id) {
        case "pushpull":
          return formatLength(lastDistance, u);
        case "rect":
          return `${formatLength(lastRect[0], u)}, ${formatLength(lastRect[1], u)}`;
        default:
          return "";
      }
    },
    [lastDistance, lastRect],
  );

  const fail = useCallback((err: unknown) => {
    const text = err instanceof Error ? err.message : String(err);
    if (err instanceof KernelUnavailableError) {
      setKernelState("missing");
      setKernelNote(text);
    } else {
      setMessage({ text, error: true });
    }
  }, []);

  /** Run one kernel request (an op, undo or redo); the scene and push target follow. */
  const run = useCallback(
    (request: (k: KernelClient) => Promise<EditResult>, op: KernelOp | null, describe: (r: EditResult) => string): Promise<boolean> => {
      const next = queue.current.then(async () => {
        const kernel = kernelRef.current;
        if (!kernel) return false;
        try {
          const r = await request(kernel);
          setScene(r.scene);
          setKernelState("ready");
          if (op) {
            setTarget((t) => nextTarget(op, r.outcome, t));
          } else {
            // Undo / redo: keep the target only if it still exists.
            setTarget((t) => (partIndex(r.scene.mesh.parts, t) >= 0 ? t : null));
          }
          setMessage({ text: describe(r), error: false });
          return true;
        } catch (err) {
          fail(err);
          return false;
        }
      });
      queue.current = next;
      return next;
    },
    [fail],
  );

  const apply = useCallback(
    (op: KernelOp, describe: (r: EditResult) => string) => run((k) => k.apply(op), op, describe),
    [run],
  );

  const undo = useCallback(
    () => run((k) => k.undo(), null, (r) => `Undone. ${r.scene.state.undo_depth} more step(s) to undo.`),
    [run],
  );
  const redo = useCallback(
    () => run((k) => k.redo(), null, (r) => `Redone. ${r.scene.state.redo_depth} more step(s) to redo.`),
    [run],
  );

  useEffect(() => {
    const k = createKernel();
    kernelRef.current = k;
    if (!k) {
      setKernelState("missing");
      setKernelNote("Web Workers are not available here.");
      return;
    }
    k.init()
      .then((s) => {
        setScene(s);
        setKernelState("ready");
      })
      .catch((err) => {
        if (err instanceof KernelUnavailableError) {
          setKernelState("missing");
          setKernelNote(err.message);
        } else {
          setKernelState("error");
          setKernelNote(`Kernel error: ${err instanceof Error ? err.message : String(err)}`);
        }
      });
    return () => {
      k.dispose();
      kernelRef.current = null;
    };
  }, []);

  const cancelGesture = useCallback(() => {
    setRectStart(null);
    setPreview(null);
  }, []);

  const selectTool = useCallback(
    (id: ToolId) => {
      setToolId(id);
      setMessage(null);
      cancelGesture();
      setVcb(vcbValueFor(id, units));
    },
    [units, vcbValueFor, cancelGesture],
  );

  const fmt = useCallback((m: number) => formatLength(m, units), [units]);
  const u = unitInfo(units);

  const drawRectangle = useCallback(
    (anchor: RectAnchor, end: [number, number] | null, size?: [number, number]) => {
      const op = rectangleOp(anchor, end, size);
      const a = toLocal(anchor.frame, anchor.point);
      const b = size ? [a[0] + size[0], a[1] + size[1]] : (end ?? a);
      const [w, d] = [Math.abs(b[0] - a[0]), Math.abs(b[1] - a[1])];
      cancelGesture();
      void apply(op, (r) => {
        const where = op.op === "imprint" ? `on face ${op.face} (imprinted, face ${r.outcome.kind === "imprinted" ? r.outcome.face : "?"})` : "on the ground";
        return `Rectangle ${fmt(w)} × ${fmt(d)} ${u.short} ${where}. Push/Pull (P) and type a distance to raise it.`;
      }).then((ok) => ok && setLastRect([w, d]));
    },
    [apply, cancelGesture, fmt, u.short],
  );

  const onCanvasClick = useCallback(
    (x: number, y: number) => {
      const engine = engineRef.current;
      const parts = scene?.mesh.parts ?? [];
      if (!engine) return;
      const hit = engine.pick(x, y);
      if (toolId === "rect") {
        if (!rectStart) {
          if (!hit) return;
          const part = hit.part >= 0 ? parts[hit.part] : null;
          const anchor: RectAnchor =
            part && part.kind === "face"
              ? { frame: frameOfPart(part), point: hit.point, face: { object: part.owner, face: part.index } }
              : { frame: GROUND, point: [hit.point[0], hit.point[1], 0], face: null };
          setRectStart(anchor);
          setMessage({ text: `First corner set ${anchor.face ? `on face ${anchor.face.face}` : "on the ground"}. Click the opposite corner, or type width, depth.`, error: false });
          return;
        }
        const { origin, dir } = engine.ray(x, y);
        const p = rayPlane(origin, dir, rectStart.frame);
        if (!p) return;
        drawRectangle(rectStart, toLocal(rectStart.frame, p));
        return;
      }
      if (toolId === "pushpull" || toolId === "select") {
        const part = hit && hit.part >= 0 ? parts[hit.part] : null;
        const t = part ? targetOfPart(part) : null;
        setTarget(t);
        setMessage(
          t
            ? { text: `Selected ${describeTarget(t)}.${toolId === "pushpull" ? " Type a distance and press Enter (negative pushes in)." : ""}`, error: false }
            : null,
        );
      }
    },
    [scene, toolId, rectStart, drawRectangle],
  );

  const onCanvasMove = useCallback(
    (x: number, y: number) => {
      if (toolId !== "rect" || !rectStart) return;
      const engine = engineRef.current;
      if (!engine) return;
      const { origin, dir } = engine.ray(x, y);
      const p = rayPlane(origin, dir, rectStart.frame);
      if (!p) return;
      const a = toLocal(rectStart.frame, rectStart.point);
      const corners = rectCorners(a, toLocal(rectStart.frame, p)).map(([cx, cy]) => toWorld(rectStart.frame, cx, cy));
      setPreview(corners);
    },
    [toolId, rectStart],
  );

  const commit = useCallback(
    (text: string) => {
      if (toolId === "pushpull") {
        const r = parseLength(text, units);
        if (!r.ok) return setMessage({ text: r.error, error: true });
        const t = targetRef.current;
        if (!t) return setMessage({ text: "Click a face or a region first, then type the distance.", error: true });
        const d = r.metres;
        void apply(pushOp(t, d), (res) => {
          const s = res.scene.state.stats;
          const what = res.outcome.kind === "object_created" ? "Extruded" : res.outcome.kind === "object_collapsed" ? "Collapsed" : d >= 0 ? "Pulled" : "Pushed";
          return `${what} ${fmt(Math.abs(d))} ${u.short}. ${s.objects} solid(s): ${s.faces} faces, ${s.edges} edges, ${s.vertices} vertices, genus ${s.genus}, ${s.watertight ? "watertight" : "OPEN"}.`;
        }).then((ok) => {
          if (ok) {
            setLastDistance(Math.abs(d));
            setVcb(fmt(Math.abs(d)));
            vcbRef.current?.blur();
          }
        });
        return;
      }
      if (toolId === "rect") {
        const r = parseDimensions(text, units);
        if (!r.ok) return setMessage({ text: r.error, error: true });
        const anchor = rectStart ?? { frame: GROUND, point: [0, 0, 0] as Vec3, face: null };
        drawRectangle(anchor, null, r.metres);
        vcbRef.current?.blur();
        return;
      }
      if (tool.vcb) setMessage({ text: `${tool.label} arrives in a later lane. This build wires Rect (R) and Push/Pull (P) to the kernel.`, error: false });
    },
    [toolId, tool, units, apply, fmt, u.short, rectStart, drawRectangle],
  );

  // Global shortcuts: tool keys, undo / redo, and "just type" into the VCB like SketchUp.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (mod && !e.altKey && !isTextEntry(e.target)) {
        const k = e.key.toLowerCase();
        if (k === "z" && !e.shiftKey) {
          e.preventDefault();
          void undo();
          return;
        }
        if (k === "y" || (k === "z" && e.shiftKey)) {
          e.preventDefault();
          void redo();
          return;
        }
      }
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      if (isTextEntry(e.target)) return;
      if (e.key === "Escape") {
        setMessage(null);
        cancelGesture();
        return;
      }
      if (tool.vcb && startsValueEntry(e.key)) {
        e.preventDefault();
        setVcb(e.key);
        vcbRef.current?.focus();
        return;
      }
      const t = toolForKey(e.key);
      if (t) {
        e.preventDefault();
        selectTool(t.id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [tool, selectTool, undo, redo, cancelGesture]);

  const cycleUnits = () => {
    const next = nextUnitSystem(units);
    setUnits(next);
    saveUnits(next);
    setVcb(vcbValueFor(toolId, next));
  };

  const st = scene?.state;
  const selection = scene ? partIndex(scene.mesh.parts, target) : -1;
  const hud: { text: string; tone?: "ok" | "err" }[] = [];
  if (kernelState === "ready" && st) {
    const s = st.stats;
    hud.push(
      s.objects > 0
        ? {
            text: `${s.objects} solid${s.objects === 1 ? "" : "s"} · ${s.faces} faces · genus ${s.genus} · ${(s.volume_m3 / M_PER_IN ** 3).toFixed(1)} in³ · ${s.watertight ? "watertight" : "open"}`,
            tone: s.watertight ? "ok" : "err",
          }
        : { text: `Sketch: ${s.regions} region${s.regions === 1 ? "" : "s"} · draw with Rect (R)` },
    );
    if (target) hud.push({ text: `Selected: ${describeTarget(target)}` });
  } else if (kernelState === "loading") {
    hud.push({ text: "Kernel loading…" });
  } else {
    hud.push({ text: kernelNote ?? `Kernel error: ${message?.text ?? "unavailable"}`, tone: "err" });
  }
  hud.push({ text: `Units: ${u.label}` });
  hud.push({ text: `Edit: Model · undo ${st?.undo_depth ?? 0} · redo ${st?.redo_depth ?? 0}` });

  return (
    <div className="app" id="app">
      <TopBar view={view} onView={setView} credits={0} />
      <ToolRail active={toolId} onSelect={selectTool} />
      <Viewport
        mesh={scene?.mesh ?? null}
        view={view}
        hud={hud}
        kernel={kernelState}
        info={st ? { hash: st.stats.hash, watertight: st.stats.watertight, objects: st.stats.objects, canUndo: st.can_undo, canRedo: st.can_redo } : undefined}
        selection={selection}
        preview={preview}
        onCanvasClick={onCanvasClick}
        onCanvasMove={onCanvasMove}
        onEngine={(e) => {
          engineRef.current = e;
        }}
      />
      <SidePanel credits={0} />
      <ValueBar
        ref={vcbRef}
        tool={tool}
        value={vcb}
        onChange={setVcb}
        onCommit={commit}
        onCancel={() => {
          setVcb(vcbValueFor(toolId, units));
          setMessage(null);
          cancelGesture();
          vcbRef.current?.blur();
        }}
        units={units}
        onCycleUnits={cycleUnits}
        message={message}
      />
    </div>
  );
}
