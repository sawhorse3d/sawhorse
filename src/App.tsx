import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { TopBar, type MenuItem } from "./components/TopBar";
import { ToolRail } from "./components/ToolRail";
import { Viewport, type KernelState, type ViewportPointer } from "./components/Viewport";
import { SidePanel } from "./components/SidePanel";
import { ValueBar } from "./components/ValueBar";
import { createKernel, KernelUnavailableError, type EditResult, type KernelClient } from "./kernel/client";
import type { Entity, KernelOp, SceneData } from "./kernel/types";
import { describeTarget, nextTarget, type Target } from "./lib/edit";
import { eraseOps, highlightFor, partsOf } from "./lib/entities";
import { startsValueEntry, toolById, toolForKey, type ToolId } from "./lib/tools";
import { formatLength, M_PER_IN, nextUnitSystem, unitInfo, UNIT_SYSTEMS, type UnitSystem } from "./lib/units";
import type { Projection, ViewId, ViewportEngine } from "./viewport/engine";
import { gridSpec, projectionForView, VIEWS } from "./viewport/camera";
import { EMPTY_VIEW, type Tool, type ToolHost, type ToolView } from "./tools/tool";
import { SelectTool } from "./tools/select";
import { EraseTool } from "./tools/erase";
import { RectTool } from "./tools/rect";
import { PushTool } from "./tools/push";
import { InferenceMemory, kernelResolver, ToolDriver } from "./tools/driver";

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
  const [units, setUnits] = useState<UnitSystem>(loadUnits);
  const [scene, setScene] = useState<SceneData | null>(null);
  const [kernelState, setKernelState] = useState<KernelState>("loading");
  const [kernelNote, setKernelNote] = useState<string | null>(null);
  const [vcb, setVcb] = useState("");
  const [message, setMessage] = useState<Message>(null);
  const [selection, setSelection] = useState<Entity[]>([]);
  const [hover, setHover] = useState<Entity | null>(null);
  const [target, setTarget] = useState<Target | null>(null);
  const [groupContext, setGroupContext] = useState<number | null>(null);
  const [toolView, setToolView] = useState<ToolView>(EMPTY_VIEW);
  // Camera: the requested standard view (re-applied when viewSeq changes), the projection,
  // zoom-extents requests, and what the camera actually shows (null after an orbit).
  const [view, setViewState] = useState<ViewId>("iso");
  const [viewSeq, setViewSeq] = useState(0);
  const [projection, setProjection] = useState<Projection>("parallel");
  const [fitSeq, setFitSeq] = useState(0);
  const [camView, setCamView] = useState<ViewId | null>("iso");
  const setView = useCallback((v: ViewId) => {
    setViewState(v);
    setViewSeq((n) => n + 1);
    setProjection(projectionForView(v));
  }, []);
  const zoomExtents = useCallback(() => setFitSeq((n) => n + 1), []);

  const vcbRef = useRef<HTMLInputElement>(null);
  const kernelRef = useRef<KernelClient | null>(null);
  const engineRef = useRef<ViewportEngine | null>(null);
  // Latest state for the tool host (tools live outside React).
  const live = useRef({ scene, units, selection, target, groupContext, vcbEditing: false });
  live.current = { ...live.current, scene, units, selection, target, groupContext };
  // Edits run one at a time, in order.
  const queue = useRef<Promise<unknown>>(Promise.resolve());

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
    (request: (k: KernelClient) => Promise<EditResult>, op: KernelOp | null, describe?: (r: EditResult) => string): Promise<EditResult | null> => {
      const next = queue.current.then(async () => {
        const kernel = kernelRef.current;
        if (!kernel) return null;
        try {
          const r = await request(kernel);
          setScene(r.scene);
          setKernelState("ready");
          const parts = r.scene.mesh.parts;
          const exists = (t: Target | null) =>
            !!t && parts.some((p) => (t.kind === "face" ? p.kind === "face" && p.owner === t.object && p.index === t.face : p.kind === "region" && p.owner === t.sketch && p.index === t.region));
          if (op) setTarget((t) => nextTarget(op, r.outcome, t));
          else setTarget((t) => (exists(t) ? t : null));
          // Drop selected entities that no longer exist (faces and regions by id).
          setSelection((sel) => sel.filter((e) => (e.kind === "face" || e.kind === "region" || e.kind === "object" ? partsOf(e, r.scene.mesh).length > 0 : true)));
          setHover(null);
          if (describe) setMessage({ text: describe(r), error: false });
          return r;
        } catch (err) {
          fail(err);
          return null;
        }
      });
      queue.current = next;
      return next;
    },
    [fail],
  );

  const apply = useCallback((op: KernelOp, describe?: (r: EditResult) => string) => run((k) => k.apply(op), op, describe), [run]);
  const undo = useCallback(() => run((k) => k.undo(), null, (r) => `Undone. ${r.scene.state.undo_depth} more step(s) to undo.`), [run]);
  const redo = useCallback(() => run((k) => k.redo(), null, (r) => `Redone. ${r.scene.state.redo_depth} more step(s) to redo.`), [run]);

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

  // ---- tools -----------------------------------------------------------------------------

  const fmt = useCallback((m: number) => formatLength(m, units), [units]);
  const u = unitInfo(units);
  const fmtRef = useRef(fmt);
  fmtRef.current = fmt;

  const toolsRef = useRef<Map<ToolId, Tool> | null>(null);
  const activeRef = useRef<Tool | null>(null);
  const host = useMemo<ToolHost>(
    () => ({
      units: () => live.current.units,
      fmt: (m) => fmtRef.current(m),
      unitLabel: () => unitInfo(live.current.units).short,
      scene: () => live.current.scene,
      mesh: () => live.current.scene?.mesh ?? null,
      state: () => live.current.scene?.state ?? null,
      apply: (op, describe) => apply(op, describe),
      status: (text, error = false) => setMessage(text === null ? null : { text, error }),
      showValue: (text) => {
        if (!live.current.vcbEditing) setVcb(text);
      },
      selection: () => live.current.selection,
      setSelection: (es) => {
        live.current.selection = es;
        setSelection(es);
      },
      hover: (e) => setHover(e),
      changed: () => setToolView(activeRef.current?.view() ?? EMPTY_VIEW),
      context: () => live.current.groupContext,
      enterGroup: (g) => {
        live.current.groupContext = g;
        setGroupContext(g);
        setSelection([]);
      },
      toScreen: (p) => engineRef.current?.worldToScreen(p) ?? [0, 0],
      target: () => live.current.target,
      setTarget: (t) => {
        live.current.target = t;
        setTarget(t);
      },
    }),
    [apply],
  );
  if (!toolsRef.current) {
    toolsRef.current = new Map<ToolId, Tool>([
      ["select", new SelectTool(host)],
      ["erase", new EraseTool(host)],
      ["rect", new RectTool(host)],
      ["pushpull", new PushTool(host)],
    ]);
  }
  const toolFor = (id: ToolId): Tool | null => toolsRef.current?.get(id) ?? null;
  const active = toolFor(toolId);
  activeRef.current = active;

  const driverRef = useRef<ToolDriver | null>(null);
  const memoryRef = useRef(new InferenceMemory());
  if (!driverRef.current) {
    driverRef.current = new ToolDriver(
      kernelResolver(
        () => kernelRef.current,
        () => engineRef.current,
        () => {
          const g = gridSpec(live.current.units);
          return [g.minor, g.major];
        },
        memoryRef.current,
        (inf) => {
          (window as unknown as { __sawhorse?: Record<string, unknown> }).__sawhorse = {
            ...((window as unknown as { __sawhorse?: Record<string, unknown> }).__sawhorse ?? {}),
            lastInference: inf,
          };
        },
      ),
      () => activeRef.current ?? (toolsRef.current!.get("select") as Tool),
    );
  }
  const onPointer = useCallback((p: ViewportPointer) => {
    if (!activeRef.current) {
      if (p.type === "up" && p.button === 0) setMessage({ text: `${toolById(toolIdRef.current).label} arrives in a later lane.`, error: false });
      return;
    }
    driverRef.current?.pointer(p);
  }, []);
  const toolIdRef = useRef(toolId);
  toolIdRef.current = toolId;

  const defaultValue = useCallback(
    (id: ToolId, uu: UnitSystem): string => {
      switch (id) {
        case "pushpull":
          return formatLength(DEFAULT_DISTANCE, uu);
        case "rect":
          return `${formatLength(DEFAULT_RECT[0], uu)}, ${formatLength(DEFAULT_RECT[1], uu)}`;
        default:
          return "";
      }
    },
    [],
  );

  const selectTool = useCallback(
    (id: ToolId) => {
      const prev = activeRef.current;
      prev?.deactivate();
      setToolId(id);
      setMessage(null);
      setHover(null);
      live.current.vcbEditing = false;
      setVcb(defaultValue(id, live.current.units));
      const next = toolFor(id);
      activeRef.current = next;
      next?.activate();
      setToolView(next?.view() ?? EMPTY_VIEW);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [defaultValue],
  );

  /**
   * Run a keyboard action after every pointer event already on its way to the tool: a fast
   * click-then-key (select, then Delete; click a face, then type a distance) must see the
   * click's result.
   */
  const afterPointer = useCallback((action: () => void): Promise<void> => {
    const d = driverRef.current;
    if (d?.pending()) return d.idle().then(action);
    action();
    return Promise.resolve();
  }, []);

  /** Delete / Backspace: erase the selection as one undo step. */
  const eraseSelection = useCallback(() => {
    const sel = live.current.selection;
    if (!sel.length) return;
    const { ops, skipped } = eraseOps(sel, live.current.scene?.state ?? null);
    if (!ops.length) {
      setMessage({ text: skipped[0] ?? "Nothing to erase.", error: true });
      return;
    }
    const op: KernelOp = ops.length === 1 ? ops[0] : { op: "batch", ops };
    void apply(op, () => `Erased ${ops.length} item${ops.length === 1 ? "" : "s"}${skipped.length ? ` (${skipped.length} skipped: ${skipped[0]})` : ""}.`).then((r) => {
      if (r) {
        live.current.selection = [];
        setSelection([]);
      }
    });
  }, [apply]);

  const selectAll = useCallback(() => {
    const m = live.current.scene?.mesh;
    if (!m) return;
    const objs = [...new Set(m.parts.filter((p) => p.kind === "face").map((p) => p.owner))].map((o) => ({ kind: "object" as const, object: o }));
    const regs = m.parts.filter((p) => p.kind === "region").map((p) => ({ kind: "region" as const, sketch: p.owner, region: p.index }));
    const all: Entity[] = [...objs, ...regs];
    live.current.selection = all;
    setSelection(all);
    setMessage({ text: `Selected ${all.length} item${all.length === 1 ? "" : "s"}.`, error: false });
  }, []);

  const commit = useCallback(
    (text: string) => {
      live.current.vcbEditing = false;
      vcbRef.current?.blur();
      if (activeRef.current?.valueLabel()) {
        // After any click still on its way to the tool (click a face, type, Enter).
        const t = activeRef.current;
        void afterPointer(() => t.onValue(text));
        return;
      }
      if (tool.vcb) setMessage({ text: `${tool.label} arrives in a later lane.`, error: false });
    },
    [tool, afterPointer],
  );

  // Global shortcuts: undo / redo (also while the Value Control Box has focus), tool keys,
  // Esc, Delete, Shift+Z, and "just type" into the VCB like SketchUp.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const inVcb = e.target === vcbRef.current;
      const mod = e.ctrlKey || e.metaKey;
      if (mod && !e.altKey && (!isTextEntry(e.target) || inVcb)) {
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
        if (k === "a" && !inVcb) {
          e.preventDefault();
          void afterPointer(selectAll);
          return;
        }
      }
      if (e.key === "Shift") activeRef.current?.onShift(true);
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      if (isTextEntry(e.target)) return;
      if (e.key === "Escape") {
        setMessage(null);
        if (activeRef.current?.cancel()) return;
        if (live.current.selection.length) {
          live.current.selection = [];
          setSelection([]);
          return;
        }
        if (live.current.groupContext !== null) {
          live.current.groupContext = null;
          setGroupContext(null);
          return;
        }
        setTarget(null);
        return;
      }
      if (e.key === "Delete" || e.key === "Backspace") {
        e.preventDefault();
        void afterPointer(eraseSelection);
        return;
      }
      if (e.shiftKey && (e.key === "Z" || e.key === "z")) {
        e.preventDefault();
        zoomExtents();
        return;
      }
      if (activeRef.current?.onKey(e)) {
        e.preventDefault();
        driverRef.current?.refresh();
        return;
      }
      if (activeRef.current?.valueLabel() && startsValueEntry(e.key)) {
        e.preventDefault();
        live.current.vcbEditing = true;
        setVcb(e.key);
        vcbRef.current?.focus();
        return;
      }
      const t = toolForKey(e.key);
      if (t) {
        e.preventDefault();
        void afterPointer(() => selectTool(t.id));
      }
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.key === "Shift") activeRef.current?.onShift(false);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("keyup", onKeyUp);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("keyup", onKeyUp);
    };
  }, [selectTool, undo, redo, zoomExtents, eraseSelection, selectAll, afterPointer]);

  const cycleUnits = () => {
    const next = nextUnitSystem(units);
    setUnits(next);
    saveUnits(next);
    live.current.units = next;
    setVcb(defaultValue(toolId, next));
  };

  const st = scene?.state;
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
    if (selection.length) hud.push({ text: `Selection: ${selection.length}` });
    else if (target && toolId === "pushpull") hud.push({ text: `Selected: ${describeTarget(target)}` });
  } else if (kernelState === "loading") {
    hud.push({ text: "Kernel loading…" });
  } else {
    hud.push({ text: kernelNote ?? `Kernel error: ${message?.text ?? "unavailable"}`, tone: "err" });
  }
  hud.push({ text: `Units: ${u.label}` });
  const groupName = groupContext !== null ? st?.groups.find((g) => g.id === groupContext)?.name : null;
  hud.push({ text: `Edit: ${groupName ? `Group ${groupName}` : "Model"} · undo ${st?.undo_depth ?? 0} · redo ${st?.redo_depth ?? 0}` });

  // Push targets show as a selection while the Push tool is active.
  const highlightEntities = useMemo<Entity[]>(() => {
    if (selection.length || toolId !== "pushpull" || !target) return selection;
    return [target.kind === "face" ? { kind: "face", object: target.object, face: target.face } : { kind: "region", sketch: target.sketch, region: target.region }];
  }, [selection, toolId, target]);
  const highlight = useMemo(() => highlightFor(highlightEntities, hover, scene?.mesh ?? null), [highlightEntities, hover, scene]);

  const menus = useMemo<Record<string, MenuItem[]>>(
    () => ({
      Edit: [
        { label: "Undo", keys: "Ctrl+Z", disabled: !st?.can_undo, onSelect: () => void undo() },
        { label: "Redo", keys: "Ctrl+Y", disabled: !st?.can_redo, onSelect: () => void redo() },
        "-",
        { label: "Select All", keys: "Ctrl+A", onSelect: selectAll },
        { label: "Select None", keys: "Esc", onSelect: () => setSelection([]) },
        { label: "Delete", keys: "Delete", disabled: !selection.length, onSelect: eraseSelection },
      ],
      View: [
        { label: "Zoom Extents", keys: "Shift+Z", onSelect: zoomExtents },
        "-",
        { label: "Perspective", checked: projection === "perspective", onSelect: () => setProjection((p) => (p === "perspective" ? "parallel" : "perspective")) },
        "-",
        ...VIEWS.map((v) => ({ label: `${v.label} view`, onSelect: () => setView(v.id) })),
      ],
    }),
    [st?.can_undo, st?.can_redo, undo, redo, zoomExtents, projection, setView, selectAll, eraseSelection, selection.length],
  );

  const valueLabel = active?.valueLabel() ?? null;

  return (
    <div className="app" id="app">
      <TopBar view={camView} onView={setView} credits={0} menus={menus} />
      <ToolRail active={toolId} onSelect={selectTool} />
      <Viewport
        mesh={scene?.mesh ?? null}
        units={units}
        view={view}
        viewSeq={viewSeq}
        projection={projection}
        fitSeq={fitSeq}
        hud={hud}
        kernel={kernelState}
        info={st ? { hash: st.stats.hash, watertight: st.stats.watertight, objects: st.stats.objects, canUndo: st.can_undo, canRedo: st.can_redo, volume: st.stats.volume_m3 } : undefined}
        highlight={highlight}
        overlay={toolView.overlay}
        marker={toolView.marker}
        labels={toolView.labels}
        box={toolView.box}
        cursor={toolView.cursor}
        selectionCount={selection.length}
        hoverKind={hover?.kind}
        onPointer={onPointer}
        onCamera={(v, p) => {
          setCamView(v);
          setProjection(p);
        }}
        onEngine={(e) => {
          engineRef.current = e;
        }}
      />
      <SidePanel credits={0} />
      <ValueBar
        ref={vcbRef}
        tool={valueLabel ? { ...tool, vcb: valueLabel } : tool}
        value={vcb}
        onChange={(v) => {
          live.current.vcbEditing = true;
          setVcb(v);
        }}
        onCommit={commit}
        onCancel={() => {
          live.current.vcbEditing = false;
          setVcb(defaultValue(toolId, units));
          setMessage(null);
          activeRef.current?.cancel();
          vcbRef.current?.blur();
        }}
        units={units}
        onCycleUnits={cycleUnits}
        message={message}
      />
    </div>
  );
}
