import { useCallback, useEffect, useRef, useState } from "react";
import { TopBar } from "./components/TopBar";
import { ToolRail } from "./components/ToolRail";
import { Viewport, type KernelState } from "./components/Viewport";
import { SidePanel } from "./components/SidePanel";
import { ValueBar } from "./components/ValueBar";
import { createKernel, KernelUnavailableError, type KernelClient } from "./kernel/client";
import type { MeshData, SpikeRecipe } from "./kernel/types";
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
import type { ViewId } from "./viewport/engine";

/** The end-to-end spike scene: a 24 x 16 in rectangle on the ground, pushed up 12 in. */
export const DEFAULT_RECIPE: SpikeRecipe = { width: 24 * M_PER_IN, depth: 16 * M_PER_IN, distance: 12 * M_PER_IN };

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

/** What the VCB shows when a tool becomes active: the value that tool would edit. */
function vcbValueFor(tool: ToolId, recipe: SpikeRecipe, units: UnitSystem): string {
  switch (tool) {
    case "pushpull":
      return formatLength(recipe.distance, units);
    case "rect":
      return `${formatLength(recipe.width, units)}, ${formatLength(recipe.depth, units)}`;
    default:
      return "";
  }
}

type Message = { text: string; error: boolean } | null;

export function App() {
  const [toolId, setToolId] = useState<ToolId>("select");
  const tool = toolById(toolId);
  const [view, setView] = useState<ViewId>("iso");
  const [units, setUnits] = useState<UnitSystem>(loadUnits);
  const [recipe, setRecipe] = useState<SpikeRecipe>(DEFAULT_RECIPE);
  const [mesh, setMesh] = useState<MeshData | null>(null);
  const [kernelState, setKernelState] = useState<KernelState>("loading");
  const [kernelNote, setKernelNote] = useState<string | null>(null);
  const [vcb, setVcb] = useState("");
  const [message, setMessage] = useState<Message>(null);
  const vcbRef = useRef<HTMLInputElement>(null);
  const kernelRef = useRef<KernelClient | null>(null);
  const seq = useRef(0);

  /** Ask the kernel for a scene; only a successful build replaces the current one. */
  const rebuild = useCallback(async (next: SpikeRecipe, describe?: (m: MeshData) => string): Promise<boolean> => {
    const kernel = kernelRef.current;
    if (!kernel) return false;
    const mine = ++seq.current;
    try {
      const m = await kernel.build(next);
      if (mine !== seq.current) return false;
      setMesh(m);
      setRecipe(next);
      setKernelState("ready");
      setKernelNote(null);
      if (describe) setMessage({ text: describe(m), error: false });
      return true;
    } catch (err) {
      if (mine !== seq.current) return false;
      const text = err instanceof Error ? err.message : String(err);
      if (err instanceof KernelUnavailableError) {
        setKernelState("missing");
        setKernelNote(text);
      } else {
        // A typed kernel refusal (e.g. zero distance): keep the last good scene.
        setMessage({ text, error: true });
        setKernelState((s) => (s === "loading" ? "error" : s));
      }
      return false;
    }
  }, []);

  useEffect(() => {
    const k = createKernel();
    kernelRef.current = k;
    if (!k) {
      setKernelState("missing");
      setKernelNote("Web Workers are not available here.");
      return;
    }
    void rebuild(DEFAULT_RECIPE);
    return () => {
      k.dispose();
      kernelRef.current = null;
    };
  }, [rebuild]);

  const selectTool = useCallback(
    (id: ToolId) => {
      setToolId(id);
      setMessage(null);
      setVcb(vcbValueFor(id, recipe, units));
    },
    [recipe, units],
  );

  const commit = useCallback(
    (text: string) => {
      const u = unitInfo(units);
      if (toolId === "pushpull") {
        const r = parseLength(text, units);
        if (!r.ok) return setMessage({ text: r.error, error: true });
        void rebuild({ ...recipe, distance: r.metres }, (m) => {
          const s = m.stats;
          return `Pushed ${formatLength(r.metres, units)} ${u.short}. Kernel solid: ${s.faces} faces, ${s.edges} edges, ${s.vertices} vertices, ${s.watertight ? "watertight" : "OPEN"}.`;
        }).then((ok) => ok && setVcb(formatLength(r.metres, units)));
        return;
      }
      if (toolId === "rect") {
        const r = parseDimensions(text, units);
        if (!r.ok) return setMessage({ text: r.error, error: true });
        const [w, d] = r.metres;
        void rebuild({ ...recipe, width: w, depth: d }, () => `Rectangle ${formatLength(w, units)} × ${formatLength(d, units)} ${u.short}, pushed ${formatLength(recipe.distance, units)}.`);
        return;
      }
      if (tool.vcb) setMessage({ text: `${tool.label} arrives in a later lane. This build wires Push/Pull (P) and Rect (R) to the kernel.`, error: false });
    },
    [toolId, tool, units, recipe, rebuild],
  );

  // Global shortcuts: tool keys, and "just type" into the VCB like SketchUp.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      if (isTextEntry(e.target)) return;
      if (e.key === "Escape") {
        setMessage(null);
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
  }, [tool, selectTool]);

  const cycleUnits = () => {
    const next = nextUnitSystem(units);
    setUnits(next);
    saveUnits(next);
    setVcb(vcbValueFor(toolId, recipe, next));
  };

  const u = unitInfo(units);
  const fmt = (m: number) => formatLength(m, units);
  const hud: { text: string; tone?: "ok" | "err" }[] = [];
  if (kernelState === "ready" && mesh) {
    hud.push({
      text: `Kernel box ${fmt(recipe.width)} × ${fmt(recipe.depth)} × ${fmt(Math.abs(recipe.distance))} ${u.short} · ${mesh.stats.watertight ? "watertight" : "open"}`,
      tone: mesh.stats.watertight ? "ok" : "err",
    });
  } else if (kernelState === "loading") {
    hud.push({ text: "Kernel loading…" });
  } else {
    hud.push({ text: kernelNote ?? `Kernel error: ${message?.text ?? "unavailable"}`, tone: "err" });
  }
  hud.push({ text: `Units: ${u.label}` });
  hud.push({ text: "Edit: Model" });

  return (
    <div className="app" id="app">
      <TopBar view={view} onView={setView} credits={0} />
      <ToolRail active={toolId} onSelect={selectTool} />
      <Viewport mesh={mesh} view={view} hud={hud} kernel={kernelState} />
      <SidePanel credits={0} />
      <ValueBar
        ref={vcbRef}
        tool={tool}
        value={vcb}
        onChange={setVcb}
        onCommit={commit}
        onCancel={() => {
          setVcb(vcbValueFor(toolId, recipe, units));
          setMessage(null);
          vcbRef.current?.blur();
        }}
        units={units}
        onCycleUnits={cycleUnits}
        message={message}
      />
    </div>
  );
}
