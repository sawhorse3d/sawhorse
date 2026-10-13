import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { TopBar, type MenuItem } from "./components/TopBar";
import { DemoBanner } from "./components/DemoBanner";
import { isWebDemo, RELEASES_URL, REPO_URL, sitePage } from "./lib/site";
import { ToolRail } from "./components/ToolRail";
import { Viewport, type KernelState, type ViewportPointer } from "./components/Viewport";
import { SidePanel, type PaneId } from "./components/SidePanel";
import { CutListReport } from "./components/CutListReport";
import { rowKey } from "./components/CutListPane";
import { NestingSection } from "./components/NestingSection";
import { StockDialog } from "./components/StockDialog";
import { nestingPdf } from "./nesting/booklet";
import { DXF_TYPE, nestingDxf, type DxfFlavour } from "./nesting/dxf";
import { fineU, pieceTitle } from "./nesting/format";
import { pieceFileName, pieceSvg, SVG_TYPE } from "./nesting/svg";
import { grainOverlay } from "./cutlist/arrows";
import { csvBytes, CSV_TYPE } from "./cutlist/csv";
import { labelsPdf, reportPdf } from "./cutlist/documents";
import { followRounding, GRAIN_WORDS, resolveRounding, sizeStyle, type RoundingChoice } from "./cutlist/format";
import { loadCutRounding, saveCutRounding } from "./cutlist/rounding-store";
import { PDF_TYPE } from "./cutlist/pdf";
import type { CutList, CutRow, Grain, Nesting, NestSettings, Placement, Rounding, StockDef, StockPiece } from "./kernel/types";
import { ValueBar, type StatusMessage } from "./components/ValueBar";
import { createKernel, KernelUnavailableError, type EditResult, type EmptySkp, type KernelClient, type RestartInfo } from "./kernel/client";
import { useRenderGate } from "./credits/gate";
import { SettingsDialog } from "./components/SettingsDialog";
import type { Entity, KernelOp, SceneData } from "./kernel/types";
import { describeTarget, nextTarget, type Target } from "./lib/edit";
import { entityKey, eraseOps, highlightFor, partsOf } from "./lib/entities";
import { contextTrail, describeNode, entityShown, graphOf, quoted, scopeEntity, solidsIn } from "./lib/scope";
import { describe } from "./tools/select";
import type { OverlayItem } from "./viewport/engine";
import { startsValueEntry, toolById, toolForKey, type ToolId } from "./lib/tools";
import { formatLength, M_PER_IN, nextUnitSystem, unitInfo, UNIT_SYSTEMS, type UnitSystem } from "./lib/units";
import type { Projection, ViewId, ViewportEngine } from "./viewport/engine";
import { gridSpec, projectionForView, VIEWS } from "./viewport/camera";
import { EMPTY_VIEW, type Tool, type ToolHost, type ToolView } from "./tools/tool";
import { SelectTool } from "./tools/select";
import { EraseTool } from "./tools/erase";
import { RectTool } from "./tools/rect";
import { PushTool } from "./tools/push";
import { MoveTool } from "./tools/move";
import { RotateTool } from "./tools/rotate";
import { ScaleTool } from "./tools/scale";
import { OffsetTool } from "./tools/offset";
import { FollowTool } from "./tools/follow";
import { LineTool } from "./tools/line";
import { CircleTool } from "./tools/circle";
import { ArcTool } from "./tools/arc";
import { DimensionTool, TapeTool } from "./tools/tape";
import { annotationView } from "./lib/annotations";
import { InferenceMemory, kernelResolver, ToolDriver } from "./tools/driver";
import { friendlyError, setNameSource } from "./lib/names";
import { autosaveEnabled, clearAutosave, clearOffered, describeAutosave, loadOffered, logIsWorth, MAX_AUTOSAVE_BYTES, saveAutosave, takeAutosave, worthRestoring, type Autosave } from "./lib/autosave";
import { ALL_EXTENSIONS, IMPORT_TYPES, SHZ, extensionOf, onTauriDrop, pickFile, readBrowserFile, saveFile, stem, type FileHandle, type PickedFile } from "./io/files";
import { EXPORTS, MTL_TYPE, textureType, type ExportChoice } from "./io/exports";
import type { ExportOptions, ImportUnit, OpOutcome } from "./kernel/types";
import { ImportReport, type Report } from "./components/ImportReport";
import { NameDialog, type NameRequest } from "./components/NameDialog";
import { SceneTabs } from "./components/SceneTabs";
import { SheetsWorkspace } from "./sheets/SheetsWorkspace";
import { ModeSwitch, type WorkMode } from "./sheets/SheetTabs";
import { runSheetExport, type SheetExportKind } from "./sheets/runExport";
import type { OutlineRow, OutlinerAction } from "./components/Outliner";
import { TAG_PALETTE, hexToRgb } from "./components/TagsPane";
import { applySelection } from "./lib/entities";
import { isEditableText, keyBelongsToControl } from "./lib/focus";
import type { Item, SceneDef } from "./kernel/types";
import { useCredits } from "./credits/useCredits";
import { storageNote } from "./credits/device";
import { NothingToRenderError, renderPasses } from "./render/passes";
import { PaintTool, type PaintContext } from "./tools/paint";
import type { ActiveRef } from "./components/MaterialsPane";
import type { LibraryEntry, MaterialInfo, Shadows } from "./kernel/types";
import { compactStyle, fullStyle, materialNameFor, sniffImage, texturePathFor, toBase64, type FullStyle } from "./lib/looks";
import { usePlugins } from "./plugins/usePlugins";
import { pickFile as pickAnyFile } from "./io/files";
import type { AppBridge } from "./plugins/manager";
// Lane 12: section planes.
import { SectionTool } from "./tools/section";
import { activeSectionHere, describeSection, sceneSectionsOf, sectionById, sliceExportRefused } from "./lib/sections";

/** The solids, groups and components a selection stands for (a face or edge stands for its
 * solid), each once, in selection order. */
export function itemsOf(es: Entity[]): Item[] {
  const out: Item[] = [];
  const seen = new Set<string>();
  const push = (it: Item) => {
    const k = JSON.stringify(it);
    if (!seen.has(k)) {
      seen.add(k);
      out.push(it);
    }
  };
  for (const e of es) {
    if (e.kind === "node") push({ node: e.node });
    else if (e.kind === "object" || e.kind === "face") push({ object: e.object });
    else if ((e.kind === "edge" || e.kind === "vertex") && e.owner.kind === "object") push({ object: e.owner.id });
  }
  return out;
}

/** A name not in `taken`: "Scene 1", "Scene 2"... */
export function nextName(stem: string, taken: string[]): string {
  for (let k = taken.length + 1; ; k++) if (!taken.includes(`${stem} ${k}`)) return `${stem} ${k}`;
}

/** The dashed box around the open group or component (nothing at the model level). */
export function contextBoxOverlay(st: import("./kernel/types").DocState | null): OverlayItem[] {
  const g = st?.graph;
  if (!st || !g || g.context === null) return [];
  const inside = new Set(solidsIn(g, g.context));
  const boxes = st.solids.filter((s) => inside.has(s.id));
  if (!boxes.length) return [];
  const lo = [0, 1, 2].map((k) => Math.min(...boxes.map((b) => b.min[k])));
  const hi = [0, 1, 2].map((k) => Math.max(...boxes.map((b) => b.max[k])));
  const c = (x: number, y: number, z: number): [number, number, number] => [x ? hi[0] : lo[0], y ? hi[1] : lo[1], z ? hi[2] : lo[2]];
  const pts: [number, number, number][] = [];
  for (const [a, b] of [
    [c(0, 0, 0), c(1, 0, 0)], [c(1, 0, 0), c(1, 1, 0)], [c(1, 1, 0), c(0, 1, 0)], [c(0, 1, 0), c(0, 0, 0)],
    [c(0, 0, 1), c(1, 0, 1)], [c(1, 0, 1), c(1, 1, 1)], [c(1, 1, 1), c(0, 1, 1)], [c(0, 1, 1), c(0, 0, 1)],
    [c(0, 0, 0), c(0, 0, 1)], [c(1, 0, 0), c(1, 0, 1)], [c(1, 1, 0), c(1, 1, 1)], [c(0, 1, 0), c(0, 1, 1)],
  ])
    pts.push(a, b);
  return [{ kind: "segments", points: pts, color: "--accent", width: 1.25, dashed: true }];
}

/** What the Value Control Box offers before anything is typed: 12 in up, 24 x 16 in rectangles. */
export const DEFAULT_DISTANCE = 12 * M_PER_IN;
export const DEFAULT_RECT: [number, number] = [24 * M_PER_IN, 16 * M_PER_IN];

const UNITS_KEY = "sawhorse.units";
/** The Cut list pane's Nesting section was left open (lane 9). */
const NEST_OPEN_KEY = "sawhorse.nestingOpen";
/** Wait this long after the last edit before nesting again (typing a kerf, dragging). */
const NEST_DEBOUNCE_MS = 200;

function loadNestOpen(): boolean {
  try {
    return localStorage.getItem(NEST_OPEN_KEY) === "1";
  } catch {
    return false;
  }
}

/** What selecting a nested piece selects. */
function entityOfItem(item: Item): Entity {
  return "node" in item ? { kind: "node", node: item.node } : { kind: "object", object: item.object };
}

function sameItem(a: Item, b: Item): boolean {
  return "node" in a ? "node" in b && a.node === b.node : "object" in b && a.object === b.object;
}

/** Model or Sheets (lane 12), per viewer. */
const MODE_KEY = "sawhorse.mode";
function loadMode(): WorkMode {
  try {
    return localStorage.getItem(MODE_KEY) === "sheets" ? "sheets" : "model";
  } catch {
    return "model";
  }
}

const IMPORT_UNITS_KEY = "sawhorse.importUnits";
const IMPORT_UNITS: { id: ImportUnit; label: string }[] = [
  { id: "mm", label: "Millimetres" },
  { id: "cm", label: "Centimetres" },
  { id: "m", label: "Metres" },
  { id: "in", label: "Inches" },
  { id: "ft", label: "Feet" },
];

function loadImportUnits(): ImportUnit {
  try {
    const v = localStorage.getItem(IMPORT_UNITS_KEY);
    if (v && IMPORT_UNITS.some((u) => u.id === v)) return v as ImportUnit;
  } catch {
    /* storage blocked */
  }
  return "mm";
}

/** "1 solid", "2 solids", "2 meshes", "3 sketches": words ending in s, x, sh or ch take "es". */
export const plural = (n: number, w: string) => `${n} ${n === 1 ? w : /(?:s|x|sh|ch)$/.test(w) ? `${w}es` : `${w}s`}`;

/** One line for the status bar after an import or open. */
/** Lane 15: the solids the Styles pane's Soften control acts on: every solid the selection
 * names (a solid, one of its faces or edges). Groups and components are opened first. */
export function softenTargetOf(selection: Entity[]): { objects: number[]; label: string } | null {
  const ids = new Set<number>();
  for (const e of selection) {
    if (e.kind === "object" || e.kind === "face") ids.add(e.object);
    else if (e.kind === "edge" && e.owner.kind === "object") ids.add(e.owner.id);
  }
  if (!ids.size) return null;
  return { objects: [...ids], label: `${ids.size} solid${ids.size === 1 ? "" : "s"} selected` };
}

export function describeImport(name: string, o: OpOutcome, graph?: import("./kernel/types").SceneGraph | null): string {
  if (o.kind === "restored") return `Opened ${name}: ${plural(o.objects, "solid")}${o.meshes ? `, ${plural(o.meshes, "mesh")} (not solid)` : ""}.`;
  if (o.kind !== "imported") return `Imported ${name}.`;
  const parts = [plural(o.objects.length, "solid")];
  if (o.meshes.length) parts.push(`${plural(o.meshes.length, "mesh")} that ${o.meshes.length === 1 ? "is" : "are"} not solid`);
  if (o.groups.length) {
    // Top-level groups and component instances (the graph tells which is which).
    const kinds = o.groups.map((id) => graph?.nodes.find((n) => n.id === id)?.kind ?? "group");
    const groups = kinds.filter((k) => k === "group").length;
    const instances = kinds.length - groups;
    if (groups) parts.push(plural(groups, "group"));
    if (instances) {
      const defs = new Set(o.groups.map((id) => graph?.nodes.find((n) => n.id === id)?.definition).filter((d) => d !== null && d !== undefined));
      parts.push(`${plural(instances, "component instance")} (${plural(defs.size, "component")})`);
    }
  }
  if (o.sketches.length) parts.push(`strokes in ${plural(o.sketches.length, "sketch")}`);
  return `Imported ${name}: ${parts.join(", ")}.`;
}

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

/** One op per part named (rows of one part, a scaled copy's, name it once); several are one
 * batch, so one Undo puts them all back. */
function partOps(rows: CutRow[], make: (row: CutRow) => KernelOp): KernelOp {
  const seen = new Set<string>();
  const ops: KernelOp[] = [];
  for (const row of rows) {
    const k = JSON.stringify(row.target);
    if (seen.has(k)) continue;
    seen.add(k);
    ops.push(make(row));
  }
  return ops.length === 1 ? ops[0] : { op: "batch", ops };
}
function partNames(rows: CutRow[]): string {
  return rows.length === 1 ? quoted(rows[0].name) : `${rows.length} parts`;
}

type Message = StatusMessage | null;

export function App() {
  const [toolId, setToolId] = useState<ToolId>("select");
  const tool = toolById(toolId);
  const [units, setUnits] = useState<UnitSystem>(loadUnits);
  const [scene, setScene] = useState<SceneData | null>(null);
  const [kernelState, setKernelState] = useState<KernelState>("loading");
  const [kernelNote, setKernelNote] = useState<string | null>(null);
  const [vcb, setVcb] = useState("");
  const [message, setMessageRaw] = useState<Message>(null);
  // Every error that reaches the status line is reworded: kernel refusals (and the file,
  // export and import errors that carry them) name things by public id, which people never
  // see (src/lib/names.ts).
  const setMessage = useCallback((m: Message) => setMessageRaw(m && m.error ? { ...m, text: friendlyError(m.text) } : m), []);
  /** After undo / redo: the active tool's gesture is void and the measurements box shows its default again. */
  const afterHistoryRef = useRef<() => void>(() => undefined);
  const [selection, setSelection] = useState<Entity[]>([]);
  const [hover, setHover] = useState<Entity | null>(null);
  const [target, setTarget] = useState<Target | null>(null);
  const [toolView, setToolView] = useState<ToolView>(EMPTY_VIEW);
  // The open document: its name, where it lives (for Save), the log length when last saved.
  const [docName, setDocName] = useState<string | null>(null);
  const fileHandle = useRef<FileHandle | undefined>(undefined);
  const [savedLogLen, setSavedLogLen] = useState<number | null>(null);
  const [report, setReport] = useState<Report | null>(null);
  const [nameRequest, setNameRequest] = useState<NameRequest | null>(null);
  const [activeScene, setActiveScene] = useState<string | null>(null);
  const [editingTag, setEditingTag] = useState<string | null>(null);
  const [focusTags, setFocusTags] = useState(0);
  const [importUnits, setImportUnits] = useState<ImportUnit>(loadImportUnits);
  // The cut list (lane 7): worked out by the kernel while its pane is open.
  const [pane, setPane] = useState<PaneId>("outliner");
  // Materials and styles (lane 10): the library, the active material, swatch images, a live
  // shadows preview while a slider moves.
  const [library, setLibrary] = useState<LibraryEntry[]>([]);
  const libraryRef = useRef<LibraryEntry[]>([]);
  libraryRef.current = library;
  const [activeMat, setActiveMatState] = useState<ActiveRef>(null);
  const activeMatRef = useRef<ActiveRef>(null);
  const setActiveMat = useCallback((a: ActiveRef) => {
    activeMatRef.current = a;
    setActiveMatState(a);
  }, []);
  const [swatches, setSwatches] = useState<Record<string, string>>({});
  const [shadowPreview, setShadowPreview] = useState<Shadows | null>(null);
  const [focusMaterials, setFocusMaterials] = useState(0);
  const [engineObj, setEngineObj] = useState<ViewportEngine | null>(null);
  // Lane 12: the model, or its drawing sheets (the 3D view stays loaded under them).
  const [mode, setModeState] = useState<WorkMode>(loadMode);
  const modeRef = useRef(mode);
  modeRef.current = mode;
  const setMode = useCallback((m: WorkMode) => {
    setModeState(m);
    try {
      localStorage.setItem(MODE_KEY, m);
    } catch {
      /* a per-viewer convenience only */
    }
  }, []);
  const cutOpen = pane === "cutlist";
  const [cutStart] = useState(loadCutRounding);
  const [cutPick, setCutPick] = useState<RoundingChoice>(cutStart.choice);
  const [cutPinned, setCutPinned] = useState(cutStart.pinned);
  const cutRounding = resolveRounding(cutPick, units);
  const cutStyle = sizeStyle(cutRounding, units);
  // The nesting writes lengths as the cut list does (lane 9).
  const nestUnits = useMemo(() => ({ r: cutRounding, style: cutStyle }), [cutRounding, cutStyle]);
  const [cutVisibleOnly, setCutVisibleOnly] = useState(false);
  const [cutList, setCutList] = useState<CutList | null>(null);
  const [cutProblem, setCutProblem] = useState<string | null>(null);
  // Nesting (lane 9): the parts laid out on stock by the kernel while its section is open.
  const [nestOpen, setNestOpen] = useState<boolean>(loadNestOpen);
  const nestShown = cutOpen && nestOpen;
  const [nesting, setNesting] = useState<Nesting | null>(null);
  const [nestProblem, setNestProblem] = useState<string | null>(null);
  const [nestBusy, setNestBusy] = useState(false);
  const [nestHover, setNestHover] = useState<Entity | null>(null);
  const [stockOpen, setStockOpen] = useState(false);
  const [dragging, setDragging] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);
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
  const live = useRef({ scene, units, selection, target, vcbEditing: false, vcbFor: null as ToolId | null, lastError: null as string | null });
  // Copy a React state value into `live` only when it changed since the last render. The
  // handlers write `live` first (imperatively) and React state after; a render that runs
  // before React has applied those updates (a keystroke renders urgently and skips the
  // edit's pending updates) must not put the stale values back: that lost the Push target
  // between "draw a rectangle" and "type a distance" when the keys came fast.
  const seen = useRef({ scene, units, selection, target });
  {
    const prev = seen.current;
    const next = { scene, units, selection, target };
    if (prev.scene !== scene) live.current.scene = scene;
    if (prev.units !== units) live.current.units = units;
    if (prev.selection !== selection) live.current.selection = selection;
    if (prev.target !== target) live.current.target = target;
    seen.current = next;
  }
  // The plugin system (lane 11; set further down, read by callbacks and toolFor).
  const pluginsRef = useRef<ReturnType<typeof usePlugins> | null>(null);
  // Edits run one at a time, in order.
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const editsPending = useRef(0);

  const fail = useCallback((err: unknown) => {
    const raw = err instanceof Error ? err.message : String(err);
    // Kernel refusals name things by public id; the status line never shows raw ids.
    const text = friendlyError(raw);
    if (err instanceof KernelUnavailableError) {
      setKernelState("missing");
      setKernelNote(text);
    } else {
      setMessage({ text, error: true });
    }
  }, []);

  // ---- dev autosave (IndexedDB): the op log after every edit; offered back on reload ----
  const autosaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  /** An autosave found at load and not yet restored or discarded (also on the Edit menu). */
  const [pendingRestore, setPendingRestore] = useState<Autosave | null>(null);
  /** Lane 16: said once per model when its history is too large to autosave. */
  const autosaveTooLarge = useRef(false);
  /** Lane 16: the log hash of the model as last saved, opened or started new. While the
   * model is still that, there is nothing to recover and no autosave is kept. */
  const cleanLogHash = useRef<string | null>(null);
  const markClean = useCallback((logHash: string | null) => {
    cleanLogHash.current = logHash;
    autosaveTooLarge.current = false;
    void clearAutosave();
  }, []);
  const scheduleAutosave = useCallback(() => {
    if (!autosaveEnabled()) return;
    if (autosaveTimer.current) clearTimeout(autosaveTimer.current);
    autosaveTimer.current = setTimeout(() => {
      const k = kernelRef.current;
      if (!k) return;
      if (cleanLogHash.current !== null && live.current.scene?.state.log_hash === cleanLogHash.current) {
        void clearAutosave();
        return;
      }
      // The client keeps the log (lane 16): no round trip to a kernel that may be busy.
      const ops = k.logTexts();
      const json = k.logJson();
      void saveAutosave(json, { savedAt: Date.now(), solids: live.current.scene?.state.stats.objects ?? 0, steps: ops.length, worth: logIsWorth(ops) })
        .then((kept) => {
          if (kept || autosaveTooLarge.current) return;
          autosaveTooLarge.current = true;
          setMessage({ text: `Autosave is off for this model: its history is larger than ${MAX_AUTOSAVE_BYTES / 1024 / 1024} MB. Save it as a file (File > Save).`, error: false });
        })
        .catch(() => undefined);
    }, 300);
  }, []);

  /** Run one kernel request (an op, undo or redo); the scene and push target follow. */
  const run = useCallback(
    (request: (k: KernelClient) => Promise<EditResult>, op: KernelOp | null, describe?: (r: EditResult) => string): Promise<EditResult | null> => {
      editsPending.current++;
      const next = queue.current.then(async () => {
        const kernel = kernelRef.current;
        if (!kernel) return null;
        try {
          const r = await request(kernel);
          // Tools read the host synchronously right after this resolves (a key waiting on
          // the edit), so the live copy is updated here, not on the next render.
          live.current.scene = r.scene;
          setNameSource(r.scene.state, r.scene.mesh);
          setScene(r.scene);
          setKernelState("ready");
          const parts = r.scene.mesh.parts;
          const exists = (t: Target | null) =>
            !!t && parts.some((p) => (t.kind === "face" ? p.kind === "face" && p.owner === t.object && p.index === t.face : p.kind === "region" && p.owner === t.sketch && p.index === t.region));
          const prev = live.current.target;
          const t = op ? nextTarget(op, r.outcome, prev) : exists(prev) ? prev : null;
          live.current.target = t;
          setTarget(t);
          // Drop selected entities that no longer exist (faces and regions by id, groups and
          // components by node), that the editing context no longer offers, or that are no
          // longer drawn (hidden, by the eye, a tag or a scene): Delete must never erase what
          // nobody can see.
          const g = r.scene.state.graph;
          const sel = live.current.selection.filter((e) => {
            if (!entityShown(e, r.scene.state)) return false;
            if (e.kind === "node") {
              if (!g?.nodes.some((n) => n.id === e.node)) return false;
            } else if ((e.kind === "face" || e.kind === "region" || e.kind === "object") && partsOf(e, r.scene.mesh).length === 0) return false;
            const s = scopeEntity(e, r.scene.state);
            return !!s && entityKey(s) === entityKey(e);
          });
          live.current.selection = sel;
          setSelection(sel);
          setHover(null);
          if (describe) setMessage({ text: describe(r), error: false });
          live.current.lastError = null;
          scheduleAutosave();
          return r;
        } catch (err) {
          live.current.lastError = friendlyError(err instanceof Error ? err.message : String(err));
          // Refusals by message, for tests and the lane report (tilted-plane boolean refusals).
          const w = window as unknown as { __sawhorse?: { refusals?: Record<string, number> } };
          w.__sawhorse = w.__sawhorse ?? {};
          const tally = (w.__sawhorse.refusals = w.__sawhorse.refusals ?? {});
          tally[live.current.lastError] = (tally[live.current.lastError] ?? 0) + 1;
          fail(err);
          return null;
        }
      });
      void next.finally(() => editsPending.current--);
      queue.current = next;
      return next;
    },
    [fail],
  );

  const apply = useCallback((op: KernelOp, describe?: (r: EditResult) => string) => run((k) => k.apply(op), op, describe), [run]);
  // Plugin batches are named undo steps (lane 11): the step is known by its depth and the
  // model hash right after it, so a name is only shown for exactly that state.
  const namedSteps = useRef<{ depth: number; hash: string; name: string }[]>([]);
  const stepName = (depth: number | undefined, hash: string | undefined) => namedSteps.current.find((n) => n.depth === depth && n.hash === hash)?.name ?? null;
  const undo = useCallback(
    () => {
      const before = live.current.scene?.state;
      const name = stepName(before?.undo_depth, before?.stats.hash);
      return run((k) => k.undo(), null, (r) => `Undone${name ? `: ${name}` : ""}. ${r.scene.state.undo_depth} more step(s) to undo.`).then((r) => {
        if (r) afterHistoryRef.current();
        return r;
      });
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [run],
  );
  const redo = useCallback(
    () =>
      run((k) => k.redo(), null, (r) => {
        const name = stepName(r.scene.state.undo_depth, r.scene.state.stats.hash);
        return `Redone${name ? `: ${name}` : ""}. ${r.scene.state.redo_depth} more step(s) to redo.`;
      }).then((r) => {
        if (r) afterHistoryRef.current();
        return r;
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [run],
  );

  const restore = useCallback(
    (a: Autosave) => {
      const k = kernelRef.current;
      if (!k) return;
      setPendingRestore(null);
      editsPending.current++;
      const next = queue.current.then(async () => {
        try {
          // The log goes to the kernel as the text it was kept as (lane 16).
          const json = await loadOffered();
          if (json === null) throw new Error("the unsaved work kept in this browser cannot be read");
          const s = await k.replay(json);
          void clearOffered();
          live.current.scene = s;
          setNameSource(s.state, s.mesh);
          setScene(s);
          live.current.target = null;
          setTarget(null);
          live.current.selection = [];
          setSelection([]);
          setMessage({ text: `Restored your unsaved work: ${s.state.stats.objects} solid${s.state.stats.objects === 1 ? "" : "s"}, ${a.steps} steps (undo works as before).`, error: false });
          setFitSeq((n) => n + 1);
          scheduleAutosave();
        } catch (err) {
          // Kept aside, still offered: Discard removes it.
          setPendingRestore({ ...a, state: "unreadable" });
          setMessage({ text: `Could not restore the unsaved work: ${friendlyError(err instanceof Error ? err.message : String(err))}`, error: true, actions: [{ label: "Discard", run: () => discardRef.current() }] });
        }
      });
      void next.finally(() => editsPending.current--);
      queue.current = next;
    },
    [scheduleAutosave],
  );
  const discardRestore = useCallback(() => {
    setPendingRestore(null);
    void clearOffered();
    setMessage({ text: "Unsaved work discarded.", error: false });
  }, []);
  const discardRef = useRef(discardRestore);
  discardRef.current = discardRestore;

  // ---- files ---------------------------------------------------------------------------

  const dirty = !!scene && savedLogLen !== null && scene.state.log_len !== savedLogLen;
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  useEffect(() => {
    document.title = `${dirty ? "• " : ""}${docName ? stem(docName) : "Untitled"} - Sawhorse`;
  }, [dirty, docName]);

  const confirmDiscard = useCallback(() => !dirtyRef.current || window.confirm("Discard the unsaved changes to this model?"), []);

  /** Bring a file into the model as one undo step (`.skp` through openskp, the rest through the kernel's readers). */
  const importBytes = useCallback(
    async (f: PickedFile) => {
      const ext = extensionOf(f.name);
      // Lane 16: a long import can be stopped (the kernel restarts from the last step).
      setMessage({ text: `Importing ${f.name}…`, error: false, actions: [{ label: "Cancel", run: () => void kernelRef.current?.cancel("cancelled by you") }] });
      let notes: string[] = [];
      let empty: EmptySkp | null = null;
      const r = await run(
        async (k) => {
          const res = ext === ".skp" ? await k.importSkp(f.name, f.bytes) : await k.importFile(f.name, f.bytes, { units: importUnits });
          notes = res.notes;
          if (!("empty" in res && res.empty)) return res as EditResult;
          // Lane 15: an empty SketchUp file (a blank template) opens as an empty model in its
          // own units and style, in place of the current one.
          empty = res.empty;
          if (!confirmDiscard()) throw new Error(`${f.name} is an empty model; the current model was kept.`);
          let n = await k.newDocument();
          const face = res.empty.style?.face;
          if (face) n = await k.apply({ op: "set_style", style: { face } } as KernelOp);
          return n;
        },
        null,
        (res) => (empty ? `Opened ${f.name}: an empty model.` : describeImport(f.name, res.outcome, res.scene.state.graph)),
      );
      if (!r) {
        setReport({ title: `Could not import ${f.name}`, lines: [live.current.lastError ?? "refused"], error: true });
        return;
      }
      if (empty) {
        const e = empty as EmptySkp;
        pluginsRef.current?.documentOpened(null);
        setDocName(null);
        fileHandle.current = undefined;
        setSavedLogLen(r.scene.state.log_len);
        if (e.units && UNIT_SYSTEMS.some((x) => x.id === e.units)) {
          setUnits(e.units as UnitSystem);
          live.current.units = e.units as UnitSystem;
        }
        setReport({ title: `Opened ${f.name}: an empty model`, lines: notes });
        setSelection([]);
        setView("iso");
        return;
      }
      const kernelNotes = r.outcome.kind === "imported" ? r.outcome.notes : [];
      setReport({ title: describeImport(f.name, r.outcome, r.scene.state.graph), lines: [...notes, ...kernelNotes] });
      const w = window as unknown as { __sawhorse?: Record<string, unknown> };
      w.__sawhorse = { ...(w.__sawhorse ?? {}), lastImport: { name: f.name, outcome: r.outcome, notes: [...notes, ...kernelNotes] } };
      setFitSeq((n) => n + 1);
    },
    [run, importUnits, confirmDiscard],
  );

  /** Open a .shz in place of the current model. */
  const openBytes = useCallback(
    async (f: PickedFile) => {
      if (!confirmDiscard()) return;
      setMessage({ text: `Opening ${f.name}…`, error: false, actions: [{ label: "Cancel", run: () => void kernelRef.current?.cancel("cancelled by you") }] });
      let info: { notes: string[]; opened: { display_units: string | null } } | null = null;
      const r = await run(
        async (k) => {
          const res = await k.openShz(f.bytes);
          info = res;
          return res;
        },
        null,
        (res) => describeImport(f.name, res.outcome, res.scene.state.graph),
      );
      if (!r || !info) {
        setReport({ title: `Could not open ${f.name}`, lines: [live.current.lastError ?? "refused"], error: true });
        return;
      }
      const opened = info as { notes: string[]; opened: { display_units: string | null; plugin_data?: string | null } };
      // The plugins' document stores came back through the kernel (lane 16): the app never
      // unzips a user's file itself.
      pluginsRef.current?.documentOpened(f.name, opened.opened.plugin_data ?? null);
      setDocName(f.name);
      fileHandle.current = f.handle;
      setSavedLogLen(r.scene.state.log_len);
      // Opened: the file is the copy to go back to (lane 16).
      markClean(r.scene.state.log_hash);
      const du = opened.opened.display_units;
      if (du && UNIT_SYSTEMS.some((x) => x.id === du)) {
        setUnits(du as UnitSystem);
        live.current.units = du as UnitSystem;
      }
      setReport(opened.notes.length ? { title: `Opened ${f.name}`, lines: opened.notes } : null);
      setSelection([]);
      setFitSeq((n) => n + 1);
    },
    [run, confirmDiscard, markClean],
  );

  /** Route files (picked, dropped): a .shz opens, anything else is imported. */
  const takeFiles = useCallback(
    async (files: PickedFile[]) => {
      for (const f of files) {
        const ext = extensionOf(f.name);
        if (ext === ".shz") await openBytes(f);
        else if (ALL_EXTENSIONS.includes(ext)) await importBytes(f);
        else setMessage({ text: `${f.name}: not a file Sawhorse can open (use .shz, .skp, STL, 3MF, glTF, OBJ, DXF or SVG).`, error: true });
      }
    },
    [openBytes, importBytes],
  );

  /** Dropped or chosen browser files, one at a time (lane 16): each is checked by its name
   * and size before it is read, and taken in before the next is read. */
  const takeBrowserFiles = useCallback(
    async (files: File[]) => {
      for (const f of files) {
        if (!ALL_EXTENSIONS.includes(extensionOf(f.name))) {
          setMessage({ text: `${f.name}: not a file Sawhorse can open (use .shz, .skp, STL, 3MF, glTF, OBJ, DXF or SVG).`, error: true });
          continue;
        }
        try {
          await takeFiles([await readBrowserFile(f)]);
        } catch (e) {
          setMessage({ text: e instanceof Error ? e.message : String(e), error: true });
        }
      }
    },
    [takeFiles],
  );

  const newDocument = useCallback(async () => {
    if (!confirmDiscard()) return;
    const r = await run((k) => k.newDocument(), null, () => "New model.");
    if (r) {
      pluginsRef.current?.documentOpened(null);
      setDocName(null);
      fileHandle.current = undefined;
      setSavedLogLen(r.scene.state.log_len);
      markClean(r.scene.state.log_hash);
      setReport(null);
      setSelection([]);
      setView("iso");
    }
  }, [run, confirmDiscard, markClean]);

  const openDialog = useCallback(async () => {
    try {
      const f = await pickFile([SHZ, ...IMPORT_TYPES]);
      if (f) await takeFiles([f]);
    } catch (e) {
      setMessage({ text: `Could not open the file: ${e instanceof Error ? e.message : String(e)}`, error: true });
    }
  }, [takeFiles]);

  const importDialog = useCallback(async () => {
    try {
      const f = await pickFile(IMPORT_TYPES);
      if (f) await importBytes(f);
    } catch (e) {
      setMessage({ text: `Could not import the file: ${e instanceof Error ? e.message : String(e)}`, error: true });
    }
  }, [importBytes]);

  const save = useCallback(
    async (as: boolean) => {
      const k = kernelRef.current;
      if (!k) return;
      try {
        await queue.current;
        const png = (await engineRef.current?.snapshotThumbnail(512)) ?? new Uint8Array();
        // Plugins' document-scoped stores ride along as plugin-data.json (lane 11), written by
        // the kernel's own writer (lane 16).
        const bytes = await k.saveShz(live.current.units, png, pluginsRef.current?.manager.doc.json() ?? "");
        const name = `${docName ? stem(docName) : "Untitled"}.shz`;
        const res = await saveFile(name, bytes, SHZ, as ? undefined : fileHandle.current);
        if (!res) return;
        setDocName(res.name);
        fileHandle.current = res.handle;
        setSavedLogLen(live.current.scene?.state.log_len ?? null);
        // Saved: this session's autosave is not needed to get the work back (lane 16).
        markClean(live.current.scene?.state.log_hash ?? null);
        setMessage({ text: `Saved ${res.name} (${(bytes.length / 1024).toFixed(0)} KB).`, error: false });
      } catch (e) {
        setMessage({ text: `Could not save: ${e instanceof Error ? e.message : String(e)}`, error: true });
      }
    },
    [docName, markClean],
  );

  /** What an export takes: the selected solids (and what is in selected groups and components), or everything visible. */
  const exportScope = useCallback((): ExportOptions["scope"] => {
    const objs = new Set<number>();
    const g = graphOf(live.current.scene?.state);
    for (const e of live.current.selection) {
      if (e.kind === "object") objs.add(e.object);
      if (e.kind === "face") objs.add(e.object);
      if (e.kind === "node") solidsIn(g, e.node).forEach((o) => objs.add(o));
    }
    return objs.size ? { objects: [...objs], meshes: [] } : undefined;
  }, []);

  /** The face or sketch region an outline export draws: the one selected, or the Push/Pull target. */
  const outlineTarget = useCallback((): ExportOptions["outline"] | null => {
    const faces = live.current.selection.filter((e) => e.kind === "face" || e.kind === "region");
    const pick = faces.length === 1 ? faces[0] : null;
    if (pick?.kind === "face") return { kind: "face", owner: pick.object, index: pick.face };
    if (pick?.kind === "region") return { kind: "region", owner: pick.sketch, index: pick.region };
    const t = live.current.target;
    if (t?.kind === "face") return { kind: "face", owner: t.object, index: t.face };
    if (t?.kind === "region") return { kind: "region", owner: t.sketch, index: t.region };
    return null;
  }, []);

  const exportAs = useCallback(
    async (c: ExportChoice) => {
      const k = kernelRef.current;
      if (!k) return;
      const base = docName ? stem(docName) : "Untitled";
      const opts: ExportOptions = { scope: exportScope() };
      if (c.needs === "outline") {
        const o = outlineTarget();
        if (!o) {
          setMessage({ text: "Select one face (or sketch region) to export its outline.", error: true });
          return;
        }
        opts.outline = o;
        opts.scope = undefined;
      }
      if (c.needs === "view") {
        const e = engineRef.current;
        if (!e) return;
        opts.view = e.viewSpec();
      }
      if (c.id === "obj") opts.mtl_file = `${base}.mtl`;
      try {
        await queue.current;
        const bytes = await k.exportFile(c.id, opts);
        const res = await saveFile(`${base}${c.type.extensions[0]}`, bytes, c.type);
        if (!res) return;
        if (c.id === "obj" && (live.current.scene?.state.materials.length ?? 0) > 0) {
          await saveFile(`${base}.mtl`, await k.exportFile("mtl", {}), MTL_TYPE);
          // The texture images the MTL names (map_Kd <file>), saved beside it (lane 10).
          const st = live.current.scene?.state;
          const used = new Set((st?.materials ?? []).filter((m) => m.held && m.texture).map((m) => m.texture as string));
          for (const t of st?.textures ?? []) {
            if (!used.has(t.path)) continue;
            const file = t.path.split("/").pop() ?? t.path;
            await saveFile(file, await k.texture(t.path), textureType(t.mime));
          }
        }
        setMessage({ text: `Exported ${res.name}: ${c.hint}.`, error: false });
      } catch (e) {
        setMessage({ text: `Export refused: ${e instanceof Error ? e.message : String(e)}`, error: true });
      }
    },
    [docName, exportScope, outlineTarget],
  );

  // Desktop drag-and-drop arrives through Tauri (see src-tauri/src/lib.rs).
  useEffect(() => {
    let off = () => {};
    void onTauriDrop(
      (files) => takeFiles(files),
      (text) => setMessage({ text, error: true }),
    ).then((f) => (off = f));
    return () => off();
  }, [takeFiles]);

  useEffect(() => {
    const k = createKernel();
    kernelRef.current = k;
    if (!k) {
      setKernelState("missing");
      setKernelNote("Web Workers are not available here.");
      return;
    }
    // Lane 16: when the kernel had to be restarted (it failed, hung or was cancelled), show
    // the document rebuilt from its log and say so.
    k.onRestart = (info: RestartInfo) => {
      if (info.scene) {
        live.current.scene = info.scene;
        setNameSource(info.scene.state, info.scene.mesh);
        setScene(info.scene);
        setKernelState("ready");
      } else {
        // Given up on (it kept failing): nothing more will run; the message says to reload.
        setKernelState("error");
      }
      const w = window as unknown as { __sawhorse?: Record<string, unknown> };
      w.__sawhorse = { ...(w.__sawhorse ?? {}), lastRestart: { reason: info.reason, error: info.error ?? null, rebuilt: !!info.scene && !info.error } };
      setMessage(
        info.error
          ? { text: `The kernel stopped (${friendlyError(info.reason)}) and the model could not be rebuilt: ${friendlyError(info.error)}`, error: true }
          : { text: `The kernel stopped (${friendlyError(info.reason)}) and was restarted from your last step: nothing was lost, and undo works as before.`, error: true },
      );
    };
    // Tests (?kerneltest=1): make the next import fail inside the kernel.
    try {
      if (new URLSearchParams(window.location.search).get("kerneltest") === "1") {
        const w = window as unknown as { __sawhorse?: Record<string, unknown> };
        w.__sawhorse = { ...(w.__sawhorse ?? {}), kernelTest: { failNextImport: (how?: string) => k.failNextImport(how), restarts: () => k.restarts } };
      }
    } catch {
      /* no URL */
    }
    k.init()
      .then(async (s) => {
        setNameSource(s.state, s.mesh);
        live.current.scene = s;
        setScene(s);
        setKernelState("ready");
        setSavedLogLen(s.state.log_len);
        if (!autosaveEnabled()) return;
        // Never blocks the start (lane 16): a record that cannot be read or is too large is
        // offered for discarding only.
        try {
          const saved = await takeAutosave();
          if (worthRestoring(saved)) setPendingRestore(saved);
        } catch {
          /* nothing kept */
        }
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

  useEffect(() => {
    if (!pendingRestore) return;
    const usable = pendingRestore.state === "ok";
    setMessage({
      text: usable ? `Restore unsaved work? ${describeAutosave(pendingRestore)}.` : `Unsaved work was kept in this browser, but ${describeAutosave(pendingRestore)}; it is set aside, not loaded.`,
      error: !usable,
      actions: usable
        ? [
            { label: "Restore", run: () => restore(pendingRestore) },
            { label: "Discard", run: discardRestore },
          ]
        : [{ label: "Discard", run: discardRestore }],
    });
  }, [pendingRestore, restore, discardRestore]);

  // Read-only hooks for e2e tests: every solid with its display number, bounds and volume,
  // and the scene graph (groups, components, the open context).
  useEffect(() => {
    const w = window as unknown as { __sawhorse?: Record<string, unknown> };
    w.__sawhorse = { ...(w.__sawhorse ?? {}), solids: () => live.current.scene?.state.solids ?? [], graph: () => live.current.scene?.state.graph ?? null };
  }, []);

  // ---- cut list (lane 7) ------------------------------------------------------------------

  // Worked out again after every edit while the pane is open (read-only; never an undo step).
  useEffect(() => {
    if (!cutOpen || !scene) return;
    const k = kernelRef.current;
    if (!k) return;
    let stale = false;
    void queue.current
      .then(() => k.cutList({ rounding: cutRounding, visible_only: cutVisibleOnly }))
      .then((l) => {
        if (stale) return;
        setCutList(l ? { ...l, style: cutStyle } : l);
        setCutProblem(null);
      })
      .catch((e) => !stale && setCutProblem(`The cut list could not be worked out: ${friendlyError(e instanceof Error ? e.message : String(e))}`));
    return () => {
      stale = true;
    };
  }, [cutOpen, scene, cutRounding, cutVisibleOnly, cutStyle]);

  // The nesting: worked out again a moment after the last edit while its section is open
  // (read-only; kerf, trim and the algorithm are document settings, so they are edits too).
  useEffect(() => {
    if (!nestShown || !scene) return;
    const k = kernelRef.current;
    if (!k) return;
    let stale = false;
    setNestBusy(true);
    const t = window.setTimeout(() => {
      void queue.current
        .then(() => k.nest({ visible_only: cutVisibleOnly }))
        .then((n) => {
          if (stale) return;
          setNesting(n);
          setNestProblem(null);
          setNestBusy(false);
        })
        .catch((e) => {
          if (stale) return;
          setNestBusy(false);
          setNestProblem(`The nesting could not be worked out: ${friendlyError(e instanceof Error ? e.message : String(e))}`);
        });
    }, NEST_DEBOUNCE_MS);
    return () => {
      stale = true;
      window.clearTimeout(t);
    };
  }, [nestShown, scene, cutVisibleOnly]);

  // Read-only hooks for tests: the cut list and the nesting for any options, straight from
  // the kernel.
  useEffect(() => {
    const w = window as unknown as { __sawhorse?: Record<string, unknown> };
    w.__sawhorse = {
      ...(w.__sawhorse ?? {}),
      cutList: async (o?: { rounding?: Rounding; visible_only?: boolean }) => {
        await queue.current;
        return kernelRef.current?.cutList({ rounding: o?.rounding ?? "sixteenth", visible_only: !!o?.visible_only }) ?? null;
      },
      nest: async (o?: { visible_only?: boolean; algorithm?: "guillotine" | "maxrects" }) => {
        await queue.current;
        return kernelRef.current?.nest(o ?? {}) ?? null;
      },
    };
  }, []);

  const setRounding = useCallback(
    (r: RoundingChoice) => {
      setCutPick(r);
      // A pinned choice follows the pick; back to Follow units unpins.
      saveCutRounding(r, cutPinned && r !== "follow");
      if (cutPinned && r === "follow") setCutPinned(false);
    },
    [cutPinned],
  );
  const pinRounding = useCallback(
    (pin: boolean) => {
      setCutPinned(pin);
      saveCutRounding(cutPick, pin);
      setMessage({ text: pin ? "Cut list rounding kept for next time." : "Cut list rounding kept for this session only.", error: false });
    },
    [cutPick],
  );

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
      lastError: () => live.current.lastError,
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
      context: () => live.current.scene?.state.graph?.context ?? null,
      enterGroup: (g) => {
        live.current.selection = [];
        setSelection([]);
        void apply({ op: "set_context", node: g }, (r) => `Editing ${describeNode(r.scene.state.graph, g)}. Esc closes it.`);
      },
      toScreen: (p) => engineRef.current?.worldToScreen(p) ?? [0, 0],
      target: () => live.current.target,
      outline: async (t) => {
        const k = kernelRef.current;
        if (!k) return [];
        try {
          return t.kind === "face" ? await k.outline("face", t.object, t.face) : await k.outline("region", t.sketch, t.region);
        } catch {
          return [];
        }
      },
      setTarget: (t) => {
        live.current.target = t;
        setTarget(t);
      },
      offsetPreview: async (source, distance) => {
        const k = kernelRef.current;
        if (!k) return "The kernel is not ready.";
        try {
          return await k.offsetPreview(source, distance);
        } catch (err) {
          return friendlyError(err instanceof Error ? err.message : String(err));
        }
      },
      pickSection: (x, y) => engineRef.current?.pickSection(x, y) ?? null,
    }),
    [apply],
  );
  /** The active material as the Paint tool and the preview see it (a library material
   * before its first use looks like one without its image). */
  const activeInfo = (): MaterialInfo | null => {
    const a = activeMatRef.current;
    if (!a) return null;
    const inModel = live.current.scene?.state.materials.find((m) => m.name === a.name);
    if (inModel) return inModel;
    if (a.from === "library") {
      const e = libraryRef.current.find((x) => x.name === a.name);
      if (e) return { name: e.name, color: e.color, texture: e.texture, held: false, size: e.size ?? undefined, stock: e.stock };
    }
    return null;
  };
  const paintCtx = useMemo<PaintContext>(
    () => ({
      active: activeInfo,
      paint: async (target, what) => {
        const m = activeInfo();
        const k = kernelRef.current;
        if (!m || !k) return false;
        const paintOp: KernelOp = { op: "paint", target, material: m.name };
        const inModel = live.current.scene?.state.materials.some((x) => x.name === m.name);
        // A library material goes into the model with the first coat: one undo step.
        let op: KernelOp = paintOp;
        if (!inModel && activeMatRef.current?.from === "library") {
          try {
            op = { op: "batch", ops: [...(await k.libraryOps(m.name)), paintOp] };
          } catch (err) {
            setMessage({ text: friendlyError(err instanceof Error ? err.message : String(err)), error: true });
            return false;
          }
        }
        const r = await apply(op, () => `Painted ${what} ${m.name}.`);
        if (r && activeMatRef.current?.from === "library") setActiveMat({ from: "model", name: m.name });
        return !!r;
      },
      sample: async (object, face) => {
        const k = kernelRef.current;
        if (!k) return null;
        await queue.current;
        const name = await k.sample(object, face).catch(() => null);
        if (name) setActiveMat({ from: "model", name });
        return name;
      },
      preview: (p) => engineRef.current?.setPaintPreview(p),
      faceUvs: async (object, face, uv) => (await kernelRef.current?.faceUvs(object, face, uv)) ?? new Float32Array(),
      setPartUvs: (part, uvs) => engineRef.current?.setPartUvs(part, uvs),
      materialOf: (part) => (part.material ? (live.current.scene?.state.materials.find((m) => m.name === part.material) ?? null) : null),
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [apply],
  );
  if (!toolsRef.current) {
    toolsRef.current = new Map<ToolId, Tool>([
      ["select", new SelectTool(host)],
      ["erase", new EraseTool(host)],
      ["line", new LineTool(host)],
      ["rect", new RectTool(host)],
      ["circle", new CircleTool(host)],
      ["arc", new ArcTool(host)],
      ["tape", new TapeTool(host)],
      ["dim", new DimensionTool(host)],
      ["pushpull", new PushTool(host)],
      ["move", new MoveTool(host)],
      ["rotate", new RotateTool(host)],
      ["scale", new ScaleTool(host)],
      ["offset", new OffsetTool(host)],
      ["follow", new FollowTool(host)],
      ["paint", new PaintTool(host, paintCtx)],
      ["section", new SectionTool(host)],
    ]);
  }
  const toolFor = (id: ToolId): Tool | null => toolsRef.current?.get(id) ?? pluginsRef.current?.toolFor(id) ?? null;
  const paintTool = toolsRef.current.get("paint") as PaintTool;
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
  // ---- plugins (lane 11) -------------------------------------------------------------------
  const docNameRef = useRef(docName);
  docNameRef.current = docName;
  const selectToolRef = useRef<(id: ToolId) => void>(() => undefined);
  const [focusPlugins, setFocusPlugins] = useState(0);
  const pluginBridge = useMemo<Omit<AppBridge, "dialog">>(
    () => ({
      scene: () => live.current.scene,
      kernel: () => kernelRef.current,
      settled: () => queue.current,
      units: () => live.current.units,
      docName: () => docNameRef.current,
      selection: () => live.current.selection,
      select: (es) => {
        live.current.selection = es;
        setSelection(es);
      },
      applyBatch: async (name, ops, plugin, signal) => {
        // Lane 16: a batch still waiting in the edit queue when its plugin stops is dropped.
        const r = await run(
          (k) => (signal?.aborted ? Promise.reject(new Error(`${plugin} stopped before "${name}" ran; nothing changed`)) : k.namedBatch(name, ops)),
          null,
          () => `${plugin}: ${name}. Ctrl+Z undoes it in one step.`,
        );
        if (!r) throw new Error(live.current.lastError ?? "refused");
        namedSteps.current = [...namedSteps.current.filter((n) => n.depth !== r.scene.state.undo_depth).slice(-199), { depth: r.scene.state.undo_depth, hash: r.scene.state.stats.hash, name }];
        return r.outcome.kind === "batch" ? r.outcome.outcomes : [r.outcome];
      },
      status: (text, kind, actions) => setMessage({ text, error: kind === "error", actions }),
      saveFile: (name, bytes, type) => saveFile(name, bytes, type),
      pickFile: (types) => pickAnyFile(types),
      activateTool: (id) => selectToolRef.current(id as ToolId),
      activeTool: () => toolIdRef.current,
      fetch: (url, init) => window.fetch(url, init),
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [run],
  );
  const showPlugins = useCallback(() => setFocusPlugins((n) => n + 1), []);
  const plugins = usePlugins({ bridge: pluginBridge, toolHost: host, scene, selection, toolId, units, showPane: showPlugins });
  pluginsRef.current = plugins;

  const onPointer = useCallback((p: ViewportPointer) => {
    if (!activeRef.current) {
      if (p.type === "up" && p.button === 0) setMessage({ text: `${toolById(toolIdRef.current).label} arrives in a later lane.`, error: false });
      return;
    }
    driverRef.current?.pointer(p);
  }, []);
  const toolIdRef = useRef(toolId);
  /** A tool switch requested by a key and not applied yet (see the keydown handler). */
  const pendingToolRef = useRef<ToolId | null>(null);
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

  afterHistoryRef.current = () => {
    const t = activeRef.current;
    t?.reset();
    setToolView(t?.view() ?? EMPTY_VIEW);
    if (!live.current.vcbEditing) setVcb(defaultValue(toolIdRef.current, live.current.units));
  };

  const selectTool = useCallback(
    (id: ToolId) => {
      const prev = activeRef.current;
      prev?.deactivate();
      setToolId(id);
      setMessage(null);
      setHover(null);
      // Typing that began for this very tool while its key was on its way ("p" then "6"
      // faster than the switch) stays; anything else gives way to the tool's default.
      if (!(live.current.vcbEditing && live.current.vcbFor === id)) {
        live.current.vcbEditing = false;
        setVcb(defaultValue(id, live.current.units));
      }
      live.current.vcbFor = null;
      const next = toolFor(id);
      activeRef.current = next;
      next?.activate();
      // Paint with nothing chosen: the Materials pane comes forward.
      if (id === "paint" && !activeMatRef.current) setFocusMaterials((n) => n + 1);
      const hint = next?.variantHint();
      if (hint) setMessage({ text: hint, error: false });
      setToolView(next?.view() ?? EMPTY_VIEW);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [defaultValue],
  );
  selectToolRef.current = selectTool;


  /** A swatch image: a library texture (fetched when the Materials pane first shows it) or
   * an image the model holds (sent once by the worker). */
  const swatchFor = useCallback(
    (s: { library?: string; texture?: string | null }): string | null => {
      if (s.library) return swatches[`lib:${s.library}`] ?? null;
      if (!s.texture) return null;
      const digest = live.current.scene?.state.textures?.find((t) => t.path === s.texture)?.digest;
      return digest ? (swatches[`tex:${digest}`] ?? null) : null;
    },
    [swatches],
  );

  /** An image a sheet shows (lane 12): the swatch made from the model's copy. */
  const sheetHref = useCallback(
    (path: string): string | null => {
      const digest = live.current.scene?.state.textures?.find((t) => t.path === path)?.digest;
      return digest ? (swatches[`tex:${digest}`] ?? null) : null;
    },
    [swatches],
  );
  /** The 3D view's camera, as a sheet viewport keeps it (lane 12). */
  const sheetCamera = useCallback(() => {
    const v = engineRef.current?.viewSpec();
    return v ? { eye: v.eye, target: v.target, up: v.up, parallel: v.ortho, fov: v.fov } : null;
  }, []);

  /** Every sheet as one PDF, or printed (lane 12; the Sheet pane writes one sheet). */
  const exportSheets = useCallback(
    (kind: SheetExportKind) => {
      const k = kernelRef.current;
      if (!k) return;
      void queue.current.then(() =>
        runSheetExport(kind, { kernel: k, sheets: live.current.scene?.sheets ?? [], active: null, docName: docName ? stem(docName) : "Untitled", message: (text, error = false) => setMessage({ text, error }) }),
      );
    },
    [docName],
  );

  /** Make a material from a PNG or JPEG and make it the active one (one undo step). */
  const importImage = useCallback(
    async (file: File) => {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const mime = sniffImage(bytes);
      if (!mime) {
        setMessage({ text: `${file.name} is not a PNG or JPEG image.`, error: true });
        return;
      }
      const st = live.current.scene?.state;
      const name = materialNameFor(file.name, (st?.materials ?? []).map((m) => m.name));
      const path = texturePathFor(file.name, mime, (st?.textures ?? []).map((t) => t.path));
      const r = await apply(
        {
          op: "batch",
          ops: [
            { op: "add_texture", path, mime, data: toBase64(bytes) },
            { op: "define_material", name, color: [200, 200, 200, 255], texture: path },
          ],
        },
        () => `Material ${quoted(name)} made from ${file.name}: one foot across. Set its real size in the Materials pane.`,
      );
      if (r) setActiveMat({ from: "model", name });
    },
    [apply, setActiveMat],
  );

  /** Position the texture on the one selected face (the Materials pane's button). */
  const selected = selection.length === 1 && selection[0].kind === "face" ? selection[0] : null;
  const positionSelected = selected
    ? () => {
        selectTool("paint");
        void paintTool.startPositioning(selected.object, selected.face);
      }
    : null;

  // The library, the first time it is wanted (the Materials pane or Paint), so it never
  // holds up the edits of someone who is not painting.
  const wantLibrary = pane === "materials" || toolId === "paint";
  useEffect(() => {
    if (kernelState !== "ready" || libraryRef.current.length || !wantLibrary) return;
    void kernelRef.current
      ?.library()
      .then(setLibrary)
      .catch(() => undefined);
  }, [kernelState, wantLibrary]);

  // Library swatches, fetched while the Materials pane is open (each once).
  useEffect(() => {
    if (pane !== "materials") return;
    const k = kernelRef.current;
    const want = library.filter((e) => e.texture && !swatches[`lib:${e.name}`]);
    if (!k || !want.length) return;
    let stale = false;
    void (async () => {
      const got: Record<string, string> = {};
      for (const e of want) {
        if (stale) return;
        try {
          const png = await k.libraryTexture(e.name);
          got[`lib:${e.name}`] = URL.createObjectURL(new Blob([png as BlobPart], { type: "image/png" }));
        } catch {
          // A missing swatch shows the flat colour.
        }
      }
      if (!stale) setSwatches((s) => ({ ...s, ...got }));
    })();
    return () => {
      stale = true;
    };
  }, [pane, library, swatches]);

  // Materials, images, the style and the sun to the viewport; images the worker just sent
  // become swatches too.
  useEffect(() => {
    const st = scene?.state;
    if (!st) return;
    const blobs = scene?.textures ?? [];
    engineObj?.setLooks({ materials: st.materials, textures: st.textures, style: st.style ?? null, shadows: shadowPreview ?? st.shadows ?? null, blobs });
    // Section planes and their cuts (lane 12), after the style that says how they show.
    engineObj?.setSections(scene?.sections ?? null, st.graph?.sections ?? []);
    const fresh = blobs.filter((b) => !swatches[`tex:${b.digest}`]);
    if (fresh.length) {
      const add: Record<string, string> = {};
      for (const b of fresh) add[`tex:${b.digest}`] = URL.createObjectURL(new Blob([b.bytes as BlobPart], { type: b.mime }));
      setSwatches((s) => ({ ...s, ...add }));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scene, shadowPreview, engineObj]);

  // Read-only hooks for tests and captures: what the viewport draws, the texture handles.
  useEffect(() => {
    const w = window as unknown as { __sawhorse?: Record<string, unknown> };
    w.__sawhorse = {
      ...(w.__sawhorse ?? {}),
      looks: () => engineRef.current?.looksState() ?? null,
      // Lane 12: what the viewport shows of the section planes, and the kernel's cuts.
      sections: () => engineRef.current?.sectionsState() ?? null,
      sectionView: () => live.current.scene?.sections ?? null,
      sectionAt: (x: number, y: number) => engineRef.current?.pickSection(x, y) ?? null,
      paint: {
        active: () => activeMatRef.current,
        positioning: () => paintTool.positioningFace(),
        handles: () => paintTool.handleScreen(),
        handlesWorld: () => paintTool.handleWorld(),
        position: (object: number, face: number) => {
          selectTool("paint");
          return paintTool.startPositioning(object, face);
        },
      },
    };
  }, [paintTool, selectTool]);

  /**
   * Run a keyboard action after every pointer event already on its way to the tool and
   * every edit already sent to the kernel: a fast click-then-key (select, then Delete;
   * draw a rectangle, then type a Push/Pull distance) must see their results.
   */
  const afterPointer = useCallback((action: () => void): Promise<void> => {
    const d = driverRef.current;
    if (d?.pending() || editsPending.current > 0) {
      // Clicks first, then the edits they started (a rectangle, then Push on what it drew);
      // later clicks wait for this action in turn.
      if (d) return d.run(async () => {
        await queue.current;
        action();
      });
      return queue.current.then(() => action());
    }
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

  // ---- groups, components, tags, scenes (lane 5) ------------------------------------------

  /** Open `parent` (null: the model) first when it is not the editing context: grouping,
   * exploding and deleting act on what is directly in the open context. */
  const inContext = useCallback(
    async (parent: number | null): Promise<boolean> => {
      const ctx = live.current.scene?.state.graph?.context ?? null;
      if (ctx === parent) return true;
      return !!(await apply({ op: "set_context", node: parent }));
    },
    [apply],
  );

  const parentOfItem = useCallback((it: Item): number | null => {
    const g = graphOf(live.current.scene?.state);
    if ("node" in it) return g.nodes.find((n) => n.id === it.node)?.parent ?? null;
    return g.objects.find((o) => o.id === it.object)?.parent ?? null;
  }, []);

  const select = useCallback((es: Entity[]) => {
    live.current.selection = es;
    setSelection(es);
  }, []);

  /** Make Group from the selection (Ctrl+G). */
  const makeGroup = useCallback(
    async (items: Item[]) => {
      if (!items.length) return void setMessage({ text: "Select the solids, groups or components to group first.", error: true });
      if (!(await inContext(parentOfItem(items[0])))) return;
      const r = await apply({ op: "make_group", items }, () => `Grouped ${plural(items.length, "item")}. Double-click the group to edit inside it.`);
      if (r?.outcome.kind === "group_created") select([{ kind: "node", node: r.outcome.group }]);
    },
    [apply, inContext, parentOfItem, select],
  );

  /** Make Component from the selection: asks for its name first. */
  const makeComponent = useCallback(
    (items: Item[]) => {
      if (!items.length) return void setMessage({ text: "Select the solids, groups or components to make a component of first.", error: true });
      const g = graphOf(live.current.scene?.state);
      const taken = g.definitions.map((d) => d.name);
      // A named group made a component keeps its name (the group itself becomes the instance).
      const only = items.length === 1 && "node" in items[0] ? g.nodes.find((n) => n.id === (items[0] as { node: number }).node) : undefined;
      const fromGroup = only?.kind === "group" && only.name && !taken.includes(only.name) ? only.name : null;
      setNameRequest({
        title: "Make Component",
        label: "Name",
        value: fromGroup ?? nextName("Component", taken),
        ok: "Make Component",
        onOk: (name) =>
          void (async () => {
            if (!(await inContext(parentOfItem(items[0])))) return;
            const r = await apply({ op: "make_component", items, name }, () => `Made component ${quoted(name)}. Copies of it (Move with Ctrl) are instances: edit one, they all change.`);
            if (r?.outcome.kind === "component_created") select([{ kind: "node", node: r.outcome.instance }]);
          })(),
      });
    },
    [apply, inContext, parentOfItem, select],
  );

  const explode = useCallback(
    async (nodes: number[]) => {
      if (!nodes.length) return void setMessage({ text: "Select a group or component to explode.", error: true });
      if (!(await inContext(parentOfItem({ node: nodes[0] })))) return;
      const ops: KernelOp[] = nodes.map((node) => ({ op: "explode", node }));
      const what = nodes.length === 1 ? "it" : `${nodes.length} groups and components`;
      const r = await apply(ops.length === 1 ? ops[0] : { op: "batch", ops }, () => `Exploded ${what}: what was inside is now loose here.`);
      if (r) {
        const outs = r.outcome.kind === "batch" ? r.outcome.outcomes : [r.outcome];
        select(outs.flatMap((o) => (o.kind === "exploded" ? [...o.objects.map((object) => ({ kind: "object" as const, object })), ...o.nodes.map((node) => ({ kind: "node" as const, node }))] : [])));
      }
    },
    [apply, inContext, parentOfItem, select],
  );

  /** File > Export > Section slice: the selected plane's (or the active one's) cut, flat, 1:1
   * in millimetres (lane 12). */
  const exportSection = useCallback(
    async (section: number | null, format: "svg-section" | "dxf-section") => {
      const k = kernelRef.current;
      if (!k) return;
      const g = live.current.scene?.state.graph;
      const picked = section ?? live.current.selection.flatMap((e) => (e.kind === "section" ? [e.section] : []))[0] ?? activeSectionHere(g)?.id ?? null;
      if (picked === null) return void setMessage({ text: "Select a section plane (or activate one) to export its slice.", error: true });
      const name = sectionById(g, picked)?.name ?? "Section";
      const svg = format === "svg-section";
      try {
        await queue.current;
        const bytes = await k.exportFile(format, { section: picked });
        const type = svg ? { description: "SVG", mime: "image/svg+xml", extensions: [".svg"] } : { description: "DXF", mime: "image/vnd.dxf", extensions: [".dxf"] };
        const res = await saveFile(`${docName ? stem(docName) : "Untitled"} ${name}${svg ? ".svg" : ".dxf"}`, bytes, type);
        if (res) setMessage({ text: `Exported ${res.name}: the slice of ${quoted(name)}, 1:1 in millimetres${svg ? ", filled" : ", closed polylines"}.`, error: false });
      } catch (e) {
        setMessage({ text: sliceExportRefused(friendlyError(e instanceof Error ? e.message : String(e)), quoted(name)), error: true });
      }
    },
    [docName],
  );

  /** One section-plane action, opening the plane's group or component first when an edit
   * needs it (lane 12). */
  const sectionAction = useCallback(
    async (action: OutlinerAction, ids: number[]) => {
      const g = live.current.scene?.state.graph;
      const one = ids[0];
      const sec = sectionById(g, one);
      if (!sec) return;
      const name = quoted(sec.name);
      switch (action) {
        case "activate":
        case "deactivate":
          void apply({ op: "set_section_active", section: one, active: action === "activate" }, () => (action === "activate" ? `Section plane ${name} active: it cuts ${sec.parent === null ? "the model" : "its group or component"}.` : `Section plane ${name} switched off.`));
          return;
        case "hide":
        case "show": {
          const ops: KernelOp[] = ids.map((section) => ({ op: "set_section_hidden", section, hidden: action === "hide" }));
          void apply(ops.length === 1 ? ops[0] : { op: "batch", ops }, () => `${action === "hide" ? "Hid" : "Showed"} ${plural(ids.length, "section plane")}${action === "hide" ? " (an active one still cuts)" : ""}.`);
          return;
        }
        case "reverse":
          if (!(await inContext(sec.parent))) return;
          void apply({ op: "flip_section", section: one }, () => `Reversed ${describeSection(g, one)}: it keeps the other side now.`);
          return;
        case "slice":
          void apply({ op: "slice_to_group", section: one }, (r) => (r.outcome.kind === "sliced" ? `Made a group from the slice of ${name}: its faces are ready to push.` : "Made a group from the slice."));
          return;
        case "export-section-svg":
        case "export-section-dxf":
          void exportSection(one, action === "export-section-svg" ? "svg-section" : "dxf-section");
          return;
        case "delete": {
          if (!(await inContext(sec.parent))) return;
          const ops: KernelOp[] = ids.map((section) => ({ op: "delete_section", section }));
          const r = await apply(ops.length === 1 ? ops[0] : { op: "batch", ops }, () => `Deleted ${plural(ids.length, "section plane")}.`);
          if (r) select([]);
          return;
        }
        default:
          return;
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [apply, exportSection],
  );

  const outlinerAction = useCallback(
    (action: OutlinerAction, rows: OutlineRow[]) => {
      const entities = rows.flatMap((r) => (r.entity ? [r.entity] : []));
      const sectionIds = entities.flatMap((e) => (e.kind === "section" ? [e.section] : []));
      if (sectionIds.length) {
        void sectionAction(action, sectionIds);
        if (sectionIds.length === entities.length) return;
      }
      const items = itemsOf(entities);
      const nodes = items.flatMap((it) => ("node" in it ? [it.node] : []));
      switch (action) {
        case "edit":
          if (nodes[0] !== undefined) void apply({ op: "set_context", node: nodes[0] }, (r) => `Editing ${describeNode(r.scene.state.graph, nodes[0])}. Esc closes it.`);
          return;
        case "group":
          void makeGroup(items);
          return;
        case "component":
          makeComponent(items);
          return;
        case "unique":
          if (nodes[0] !== undefined) void apply({ op: "make_unique", node: nodes[0] }, () => "Made unique: this copy has its own definition now; editing it changes no other copy.");
          return;
        case "explode":
          void explode(nodes);
          return;
        case "hide":
        case "show":
          void apply({ op: "set_hidden", items, hidden: action === "hide" }, () => `${action === "hide" ? "Hid" : "Showed"} ${plural(items.length, "item")}.`);
          return;
        case "lock":
        case "unlock":
          void apply({ op: "set_locked", items, locked: action === "lock" }, () => `${action === "lock" ? "Locked" : "Unlocked"} ${plural(items.length, "item")}.`);
          return;
        case "delete": {
          const parents = new Set(items.map(parentOfItem));
          if (parents.size !== 1) return void setMessage({ text: "Delete things from one group or component at a time.", error: true });
          void (async () => {
            if (!(await inContext(parentOfItem(items[0])))) return;
            const ops: KernelOp[] = items.map((it) => ("node" in it ? { op: "delete_node", node: it.node } : { op: "delete_object", object: it.object }));
            const r = await apply(ops.length === 1 ? ops[0] : { op: "batch", ops }, () => `Deleted ${plural(items.length, "item")}.`);
            if (r) select([]);
          })();
          return;
        }
      }
    },
    [apply, makeGroup, makeComponent, explode, inContext, parentOfItem, select],
  );

  /** A row clicked in the Outliner: select it, opening what holds it when that is not open. */
  const outlinerSelect = useCallback(
    (row: OutlineRow, mode: "replace" | "add" | "toggle") => {
      const e = row.entity;
      if (!e) return;
      const ctx = live.current.scene?.state.graph?.context ?? null;
      if (row.parent === ctx) {
        select(applySelection(live.current.selection, [e], mode));
        setMessage({ text: describe(e, live.current.scene?.state), error: false });
        return;
      }
      void apply({ op: "set_context", node: row.parent }).then((r) => {
        if (!r) return;
        select([e]);
        setMessage({ text: describe(e, r.scene.state), error: false });
      });
    },
    [apply, select],
  );

  const outlinerRename = useCallback(
    (row: OutlineRow, name: string | null) => {
      const e = row.entity;
      if (!e) return;
      if (e.kind === "section") {
        if (name) void apply({ op: "rename_section", section: e.section, name }, () => `Section plane renamed to ${quoted(name)}.`);
        return;
      }
      const g = graphOf(live.current.scene?.state);
      if (e.kind === "node") {
        const n = g.nodes.find((x) => x.id === e.node);
        // A component is renamed as a whole (every copy shows the new name); a group by itself.
        if (n?.kind === "instance" && n.definition !== null && name) {
          void apply({ op: "rename_definition", definition: n.definition, name }, () => `Renamed the component to ${quoted(name)}.`);
          return;
        }
        void apply({ op: "rename", item: { node: e.node }, name }, () => (name ? `Renamed to ${quoted(name)}.` : "Name cleared."));
        return;
      }
      if (e.kind === "object") void apply({ op: "rename", item: { object: e.object }, name }, () => (name ? `Renamed to ${quoted(name)}.` : "Name cleared."));
    },
    [apply],
  );

  /** The tag just made with New tag, while its name is being typed: naming it reads "created". */
  const createdTagRef = useRef<string | null>(null);
  const createTag = useCallback(async () => {
    const tags = Object.keys(live.current.scene?.state.tags ?? {});
    const name = nextName("Tag", tags);
    const color = hexToRgb(TAG_PALETTE[tags.length % TAG_PALETTE.length]);
    const r = await apply({ op: "create_tag", name, color }, () => `New tag ${quoted(name)}: type its name.`);
    if (r) {
      createdTagRef.current = name;
      setEditingTag(name);
      setFocusTags((n) => n + 1);
    }
  }, [apply]);

  /** The current camera as a scene (with the hidden tags and the open context). */
  const sceneHere = useCallback((name: string): SceneDef | null => {
    const e = engineRef.current;
    const st = live.current.scene?.state;
    if (!e || !st) return null;
    const v = e.viewSpec();
    const dist = Math.hypot(v.eye[0] - v.target[0], v.eye[1] - v.target[1], v.eye[2] - v.target[2]) || 1;
    // A parallel view keeps its zoom as the field of view a perspective camera at that
    // distance would have (what lookAt reads back).
    const fov = v.ortho ? (2 * Math.atan(v.half_height / dist) * 180) / Math.PI : v.fov;
    return {
      name,
      eye: v.eye,
      target: v.target,
      up: v.up,
      fov,
      parallel: v.ortho,
      hidden_tags: Object.entries(st.tags)
        .filter(([, vis]) => !vis)
        .map(([t]) => t),
      context: st.graph?.context ?? null,
      // The style and the sun go with the scene (lane 10).
      style: st.style ?? null,
      shadows: st.shadows ?? null,
      // And which section planes are active and hidden (lane 12).
      ...(st.graph?.sections?.length ? { sections: sceneSectionsOf(st.graph) } : {}),
    };
  }, []);

  const goScene = useCallback(
    (name: string) => {
      const sc = live.current.scene?.state.scenes.find((s) => s.name === name);
      if (!sc) return;
      const p: Projection = sc.parallel ? "parallel" : "perspective";
      engineRef.current?.lookAt(sc.eye, sc.target, sc.up, p, sc.fov ?? 35);
      setProjection(p);
      setCamView(null);
      setActiveScene(name);
      void apply({ op: "activate_scene", name }, () => `Scene ${quoted(name)}${sc.hidden_tags?.length ? `: hides ${sc.hidden_tags.map(quoted).join(", ")}` : ""}.`);
    },
    [apply],
  );

  const addScene = useCallback(() => {
    const taken = (live.current.scene?.state.scenes ?? []).map((s) => s.name);
    const sc = sceneHere(nextName("Scene", taken));
    if (!sc) return;
    void apply({ op: "add_scene", scene: sc }, () => `Saved scene ${quoted(sc.name)}: this view, ${sc.hidden_tags?.length ? `hiding ${sc.hidden_tags.map(quoted).join(", ")}` : "every tag shown"}. Right-click its tab to update it.`).then((r) => r && setActiveScene(sc.name));
  }, [apply, sceneHere]);

  const updateScene = useCallback(
    (name: string) => {
      const sc = sceneHere(name);
      if (sc) void apply({ op: "update_scene", name, scene: sc }, () => `Scene ${quoted(name)} updated to this view.`).then((r) => r && setActiveScene(name));
    },
    [apply, sceneHere],
  );

  const renameScene = useCallback(
    (name: string, next: string) => {
      const sc = live.current.scene?.state.scenes.find((s) => s.name === name);
      if (sc) void apply({ op: "update_scene", name, scene: { ...sc, name: next } }, () => `Scene ${quoted(name)} renamed to ${quoted(next)}.`).then((r) => r && activeScene === name && setActiveScene(next));
    },
    [apply, activeScene],
  );

  const deleteScene = useCallback(
    (name: string) => {
      void apply({ op: "delete_scene", name }, () => `Scene ${quoted(name)} deleted.`).then((r) => r && activeScene === name && setActiveScene(null));
    },
    [apply, activeScene],
  );

  /** Cut list rows picked (a click, Shift for a range, Ctrl to toggle): select their drawn
   * pieces, opening what holds them when they share a closed group or component (as the
   * Outliner does); pieces in several places select what holds each of them in the open
   * context. No rows clears the selection. */
  const selectCutRows = useCallback(
    (rows: CutRow[]) => {
      const state = live.current.scene?.state;
      const g = graphOf(state);
      if (!rows.length) {
        select([]);
        return;
      }
      const pieces: Entity[] = rows.flatMap((row) => row.occurrences.filter((o) => o.visible).map((o): Entity => ("node" in o.item ? { kind: "node", node: o.item.node } : { kind: "object", object: o.item.object })));
      const names = rows.length === 1 ? quoted(rows[0].name) : `${rows.length} parts`;
      if (!pieces.length) return void setMessage({ text: rows.length === 1 ? `Every '${rows[0].name}' is hidden: show it (the Outliner's eye or its tag) to select it.` : `Every piece of those ${rows.length} parts is hidden: show them to select them.`, error: true });
      const parentOf = (e: Entity) => (e.kind === "node" ? (g.nodes.find((n) => n.id === e.node)?.parent ?? null) : e.kind === "object" ? (g.objects.find((o) => o.id === e.object)?.parent ?? null) : null);
      const parents = new Set(pieces.map(parentOf));
      const what = `${plural(pieces.length, "piece")} of ${names}`;
      if (parents.size === 1) {
        const p = [...parents][0];
        if (p === g.context) {
          select(pieces);
          setMessage({ text: `Selected ${what}.`, error: false });
          return;
        }
        void apply({ op: "set_context", node: p }).then((r) => {
          if (!r) return;
          select(pieces);
          setMessage({ text: `Selected ${what} (opened ${describeNode(r.scene.state.graph, p as number)} to reach them).`, error: false });
        });
        return;
      }
      const seen = new Set<string>();
      const holders: Entity[] = [];
      for (const e of pieces) {
        const s = scopeEntity(e, state);
        if (s && !seen.has(entityKey(s))) {
          seen.add(entityKey(s));
          holders.push(s);
        }
      }
      select(holders);
      setMessage({ text: `The ${what} are in ${holders.length === 1 ? "one group or component" : `${holders.length} groups or components`}; selected ${holders.length === 1 ? "it" : "those"}.`, error: false });
    },
    [apply, select],
  );

  /** Rows whose drawn pieces are all selected, or held by something selected (a row picked
   * along with rows elsewhere selects the groups or components holding its pieces). */
  const cutSelected = useMemo(() => {
    const keys = new Set(selection.map(entityKey));
    const g = graphOf(scene?.state);
    const held = (e: Entity): boolean => {
      if (keys.has(entityKey(e))) return true;
      let at = e.kind === "node" ? (g.nodes.find((n) => n.id === e.node)?.parent ?? null) : e.kind === "object" ? (g.objects.find((o) => o.id === e.object)?.parent ?? null) : null;
      for (let guard = 0; at !== null && guard < 64; guard++) {
        if (keys.has(entityKey({ kind: "node", node: at }))) return true;
        at = g.nodes.find((n) => n.id === at)?.parent ?? null;
      }
      return false;
    };
    const out = new Set<string>();
    if (!keys.size) return out;
    for (const row of cutList?.rows ?? []) {
      const vis = row.occurrences.filter((o) => o.visible);
      if (vis.length && vis.every((o) => held("node" in o.item ? { kind: "node", node: o.item.node } : { kind: "object", object: o.item.object }))) out.add(rowKey(row));
    }
    return out;
  }, [selection, cutList, scene]);

  const setGrain = useCallback(
    (rows: CutRow[], grain: Grain) => void apply(partOps(rows, (row) => ({ op: "set_grain", part: row.target, grain })), () => `Grain of ${partNames(rows)}: ${GRAIN_WORDS[grain]}.`),
    [apply],
  );
  /** `stock`: the text to set, null to use the material's name, or "keep" for each part's own
   * (marking sheet goods or lumber leaves the stock text alone). */
  const setStock = useCallback(
    (rows: CutRow[], stock: string | null | "keep", sheet: boolean | null) => {
      const own = (row: CutRow) => (stock === "keep" ? (row.stock_set ? row.material : null) : stock);
      const who = partNames(rows);
      const are = rows.length === 1 ? "is" : "are";
      void apply(partOps(rows, (row) => ({ op: "set_stock", part: row.target, stock: own(row), sheet })), () =>
        stock && stock !== "keep"
          ? `${who} ${are} cut from ${stock}${sheet === null ? "" : sheet ? " (sheet goods)" : " (solid lumber)"}.`
          : sheet === null
            ? `${who} ${rows.length === 1 ? "uses its" : "use their"} material's name again.`
            : `${who} counted as ${sheet ? "sheet goods" : "solid lumber"}.`,
      );
    },
    [apply],
  );

  /** Lane 15: count parts, or leave them out of the totals and the nesting (one undo step). */
  const setInclude = useCallback(
    (rows: CutRow[], include: boolean) =>
      void apply(partOps(rows, (row) => ({ op: "set_include", part: row.target, include })), () =>
        include ? `${partNames(rows)} counted in the totals again.` : `${partNames(rows)} left out of the totals and the nesting (still listed; tick it to count it again).`,
      ),
    [apply],
  );
  /** Lane 15: each connected piece of the loose faces in the open context becomes a group, so a part. */
  const groupLoose = useCallback(
    () =>
      void apply({ op: "group_loose", meshes: [] }, (out) => {
        const n = out.outcome.kind === "loose_grouped" ? out.outcome.groups.length : 0;
        return `Loose faces made into ${n} part${n === 1 ? "" : "s"} (one group per connected piece; Ctrl+Z puts them back).`;
      }),
    [apply],
  );

  /** Lane 15 fixes: open the group or instance holding faces outside parts (null: the top level). */
  const openLoosePlace = useCallback(
    (node: number | null) => {
      live.current.selection = [];
      setSelection([]);
      void apply({ op: "set_context", node }, (r) =>
        node === null ? "At the top level. Make parts from loose faces now takes the faces here." : `Editing ${describeNode(r.scene.state.graph, node)}. Make parts from loose faces now takes the faces here; Esc closes it.`,
      );
    },
    [apply],
  );

  // ---- nesting (lane 9) -------------------------------------------------------------------

  const toggleNest = useCallback(() => {
    const next = !nestOpen;
    setNestOpen(next);
    if (!next) {
      setNestHover(null);
      setNestBusy(false);
    }
    try {
      localStorage.setItem(NEST_OPEN_KEY, next ? "1" : "0");
    } catch {
      /* per-viewer convenience only */
    }
  }, [nestOpen]);
  const setNestSettings = useCallback(
    (s: NestSettings) =>
      void apply({ op: "set_nest_settings", settings: s }, () => `Nesting: kerf ${fineU(s.kerf, nestUnits)}, trim ${s.trim > 0 ? `${fineU(s.trim, nestUnits)} per edge` : "none"}, ${s.algorithm === "guillotine" ? "table saw (straight cuts edge to edge)" : "tightest layout"}.`),
    [apply, nestUnits],
  );
  const defineStock = useCallback(
    (stock: StockDef, replace: string | null) =>
      void apply({ op: "define_stock", stock, replace }, () => (replace ? `Stock ${quoted(replace)} saved${replace !== stock.name ? ` as ${quoted(stock.name)}` : ""}.` : `Stock ${quoted(stock.name)} added.`)),
    [apply],
  );
  const deleteStock = useCallback((name: string) => void apply({ op: "delete_stock", name }, () => `Stock ${quoted(name)} deleted.`), [apply]);
  const setMaterialStock = useCallback(
    (material: string, stock: string | null) =>
      void apply({ op: "set_material_stock", material, stock }, () => (stock ? `${quoted(material)} parts come from ${quoted(stock)}.` : `${quoted(material)} parts: stock chosen automatically.`)),
    [apply],
  );
  const resetStock = useCallback(() => void apply({ op: "reset_stock_library" }, () => "Stock back to the built-in sheets and boards (kerf 1/8 in, no trim, table saw)."), [apply]);
  const hoverPlacement = useCallback((p: Placement | null) => setNestHover(p ? entityOfItem(p.item) : null), []);
  /** A nested piece clicked: select it in the model (as its cut list row would, one piece). */
  const selectPlacement = useCallback(
    (p: Placement) => {
      const row = cutList?.rows.find((r) => r.occurrences.some((o) => sameItem(o.item, p.item)));
      if (row) selectCutRows([{ ...row, occurrences: row.occurrences.filter((o) => sameItem(o.item, p.item)) }]);
      else select([entityOfItem(p.item)]);
    },
    [cutList, selectCutRows, select],
  );
  // The part under the pointer in the viewport lights up in its diagram.
  const nestLit = useMemo(() => {
    if (!nesting || !hover || !nestShown) return null;
    const g = graphOf(scene?.state);
    const object = hover.kind === "face" || hover.kind === "object" ? hover.object : null;
    const ancestors = new Set<number>();
    if (hover.kind === "node") ancestors.add(hover.node);
    let at = object !== null ? (g.objects.find((o) => o.id === object)?.parent ?? null) : hover.kind === "node" ? (g.nodes.find((x) => x.id === hover.node)?.parent ?? null) : null;
    for (let guard = 0; at !== null && guard < 64; guard++) {
      ancestors.add(at);
      const here: number = at;
      at = g.nodes.find((x) => x.id === here)?.parent ?? null;
    }
    for (const piece of nesting.pieces)
      for (const q of piece.placements) {
        if ("object" in q.item ? q.item.object === object : ancestors.has(q.item.node)) return q.tag;
      }
    return null;
  }, [nesting, hover, nestShown, scene]);
  const nestMaterials = useMemo(() => {
    const seen = new Map<string, string>();
    for (const r of cutList?.rows ?? []) if (r.material && !seen.has(r.material.toLowerCase())) seen.set(r.material.toLowerCase(), r.material);
    for (const m of Object.keys(nesting?.library.defaults ?? {})) if (!seen.has(m.toLowerCase())) seen.set(m.toLowerCase(), m);
    return [...seen.values()];
  }, [cutList, nesting]);

  const cutTitle = docName ? stem(docName) : "Untitled";
  const saveCut = useCallback(
    async (make: (l: CutList) => Uint8Array, suffix: string, type: typeof PDF_TYPE, what: string) => {
      const l = cutList;
      if (!l) return;
      try {
        const res = await saveFile(`${cutTitle} ${suffix}`, make(l), type);
        if (res) setMessage({ text: `Saved ${res.name}: ${what}.`, error: false });
      } catch (e) {
        setMessage({ text: `Could not save the cut list: ${e instanceof Error ? e.message : String(e)}`, error: true });
      }
    },
    [cutList, cutTitle],
  );
  const saveNestSvg = useCallback(
    async (piece: StockPiece | null) => {
      const n = nesting;
      if (!n) return;
      const pieces = piece ? [piece] : n.pieces;
      let saved = 0;
      try {
        for (const pc of pieces) {
          const res = await saveFile(pieceFileName(cutTitle, pc), new TextEncoder().encode(pieceSvg(n, pc, { title: cutTitle, rounding: nestUnits })), SVG_TYPE);
          if (!res) break;
          saved += 1;
        }
        if (saved) setMessage({ text: `Saved ${saved === 1 ? `the cutting diagram of ${pieceTitle(pieces[0])}` : `${saved} cutting diagrams, one per sheet or board`} as SVG (1:1 in mm).`, error: false });
      } catch (e) {
        setMessage({ text: `Could not save the cutting diagrams: ${e instanceof Error ? e.message : String(e)}`, error: true });
      }
    },
    [nesting, cutTitle, nestUnits],
  );
  const saveNestDxf = useCallback(
    async (flavour: DxfFlavour) => {
      const n = nesting;
      if (!n) return;
      try {
        const res = await saveFile(`${cutTitle} nesting${flavour === "r12" ? " R12" : ""}.dxf`, new TextEncoder().encode(nestingDxf(n, flavour)), DXF_TYPE);
        if (res) setMessage({ text: `Saved ${res.name}: ${n.pieces.length} ${n.pieces.length === 1 ? "sheet or board" : "sheets and boards"}, a layer each, a polyline per part, in millimetres.`, error: false });
      } catch (e) {
        setMessage({ text: `Could not save the DXF: ${e instanceof Error ? e.message : String(e)}`, error: true });
      }
    },
    [nesting, cutTitle],
  );
  const saveNestPdf = useCallback(async () => {
    const n = nesting;
    if (!n) return;
    try {
      const res = await saveFile(`${cutTitle} nesting.pdf`, nestingPdf(n, cutTitle, nestUnits), PDF_TYPE);
      if (res) setMessage({ text: `Saved ${res.name}: what to buy, then a page per sheet or board with its diagram and cut sequence.`, error: false });
    } catch (e) {
      setMessage({ text: `Could not save the nesting booklet: ${e instanceof Error ? e.message : String(e)}`, error: true });
    }
  }, [nesting, cutTitle, nestUnits]);

  const printCut = useCallback(() => {
    const body = document.body;
    const done = () => {
      body.classList.remove("print-cutlist");
      window.removeEventListener("afterprint", done);
    };
    body.classList.add("print-cutlist");
    window.addEventListener("afterprint", done);
    setMessage({ text: "Printing the cut list (the report only, not the window).", error: false });
    window.print();
  }, []);

  const selectAll = useCallback(() => {
    const m = live.current.scene?.mesh;
    const state = live.current.scene?.state;
    if (!m) return;
    // Everything the open context offers: its solids, the groups and components directly
    // in it (as units), and its sketch regions.
    const seen = new Set<string>();
    const all: Entity[] = [];
    const add = (raw: Entity) => {
      const e = scopeEntity(raw, state);
      if (e && !seen.has(entityKey(e))) {
        seen.add(entityKey(e));
        all.push(e);
      }
    };
    for (const o of new Set(m.parts.filter((p) => p.kind === "face").map((p) => p.owner))) add({ kind: "object", object: o });
    for (const p of m.parts) if (p.kind === "region") add({ kind: "region", sketch: p.owner, region: p.index });
    live.current.selection = all;
    setSelection(all);
    setMessage({ text: `Selected ${all.length} item${all.length === 1 ? "" : "s"}.`, error: false });
  }, []);

  const commit = useCallback(
    (text: string) => {
      live.current.vcbEditing = false;
      vcbRef.current?.blur();
      // After any click or tool switch still on its way (click a face, press P, type, Enter):
      // the value goes to whichever tool is active by then.
      void afterPointer(() => {
        const t = activeRef.current;
        if (t?.valueLabel()) t.onValue(text);
        else {
          const def = toolById(toolIdRef.current);
          if (def.vcb) setMessage({ text: `${def.label} arrives in a later lane.`, error: false });
        }
      });
    },
    [tool, afterPointer],
  );

  // Global shortcuts: undo / redo (also while the Value Control Box has focus), tool keys,
  // Esc, Delete, Shift+Z, and "just type" into the VCB like SketchUp.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const inVcb = e.target === vcbRef.current;
      // Focus rules: the Value Control Box only owns the keyboard while something is being
      // typed into it; Ctrl+Z / Ctrl+Y always step the document, even mid-entry.
      const textEntry = keyBelongsToControl(e.target, e.key) && !(inVcb && !live.current.vcbEditing);
      const mod = e.ctrlKey || e.metaKey;
      // Sheets mode (lane 12) has its own keys (src/sheets/SheetsWorkspace.tsx); only undo,
      // redo and the file shortcuts are the app's there.
      const sheetsMode = modeRef.current === "sheets";
      // A plugin's shortcut (always with Ctrl, Alt or Meta; built-in keys win, see usePlugins).
      if (!sheetsMode && !textEntry && !inVcb && pluginsRef.current?.shortcut(e)) return;
      // Undo, redo and the file shortcuts are the app's wherever focus is (a pane's select,
      // checkbox or button too), unless a text field is being edited (it has its own undo).
      if (mod && !e.altKey && (!isEditableText(e.target) || inVcb)) {
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
        if (k === "s") {
          e.preventDefault();
          void save(e.shiftKey);
          return;
        }
        if (k === "o" && !e.shiftKey) {
          e.preventDefault();
          void openDialog();
          return;
        }
        if (k === "i" && !e.shiftKey) {
          e.preventDefault();
          void importDialog();
          return;
        }
        if (sheetsMode) return;
        if (k === "a" && !inVcb) {
          e.preventDefault();
          void afterPointer(selectAll);
          return;
        }
        if (k === "g" && !inVcb) {
          e.preventDefault();
          void afterPointer(() => {
            const sel = live.current.selection;
            if (e.shiftKey) void explode(sel.flatMap((x) => (x.kind === "node" ? [x.node] : [])));
            else void makeGroup(itemsOf(sel));
          });
          return;
        }
      }
      if (sheetsMode) return;
      if (e.key === "Shift" && !textEntry) activeRef.current?.onShift(true);
      if ((e.key === "Control" || e.key === "Alt") && !e.repeat && !textEntry) {
        // A tap of Ctrl or Alt alone (Move / Rotate copy, Move autofold). Alt must not
        // focus the browser's menu bar.
        if (e.key === "Alt") e.preventDefault();
        // After any click or tool key still on its way: the tap belongs to the tool that is
        // active by then ("s" then Ctrl scales about the centre).
        const mod = e.key === "Control" ? "ctrl" : "alt";
        void afterPointer(() => activeRef.current?.onModifier(mod));
        return;
      }
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      if (textEntry) return;
      if (inVcb && (e.key === "Enter" || e.key === "Escape")) return; // the box's own keys
      if (e.key === "Escape") {
        setMessage(null);
        if (activeRef.current?.cancel()) return;
        if (live.current.selection.length) {
          live.current.selection = [];
          setSelection([]);
          return;
        }
        if ((live.current.scene?.state.graph?.context ?? null) !== null) {
          void apply({ op: "leave_context" }, (r) => `Closed it. Editing ${contextTrail(r.scene.state.graph)}.`);
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
      if (e.key.startsWith("Arrow")) {
        // Locks act on the gesture the clicks before them started: wait for those clicks.
        e.preventDefault();
        void afterPointer(() => {
          if (activeRef.current?.onKey(e)) driverRef.current?.refresh();
        });
        return;
      }
      if (activeRef.current?.onKey(e)) {
        e.preventDefault();
        driverRef.current?.refresh();
        return;
      }
      // A tool key may still be on its way (it waits for clicks and edits in flight): typing
      // right after it belongs to that tool, so "r" then "24,16" never loses the "2".
      const typingFor = pendingToolRef.current ? toolFor(pendingToolRef.current) : activeRef.current;
      if (typingFor?.valueLabel() && startsValueEntry(e.key)) {
        e.preventDefault();
        live.current.vcbEditing = true;
        live.current.vcbFor = pendingToolRef.current;
        setVcb(e.key);
        // The box may still be read-only (a tool without values was active a moment ago and
        // React has not re-rendered yet): open it now so the next keys land in it.
        const input = vcbRef.current;
        if (input) {
          input.readOnly = false;
          input.value = e.key;
          input.focus();
        }
        return;
      }
      const t = toolForKey(e.key);
      if (t) {
        e.preventDefault();
        if (t.id !== toolIdRef.current) pendingToolRef.current = t.id;
        void afterPointer(() => {
          if (pendingToolRef.current === t.id) pendingToolRef.current = null;
          // The active tool's key again switches its variant (Rect / Rotated Rect, ...).
          if (t.id === toolIdRef.current && activeRef.current) {
            const hint = activeRef.current.cycleVariant();
            if (hint) {
              setMessage({ text: hint, error: false });
              setToolView(activeRef.current.view());
              return;
            }
          }
          selectTool(t.id);
        });
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
  }, [selectTool, undo, redo, zoomExtents, eraseSelection, selectAll, afterPointer, save, openDialog, importDialog, makeGroup, explode]);

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
    if (st.meshes?.length) hud.push({ text: `${plural(st.meshes.length, "mesh")} not solid (imported)`, tone: "err" });
    if (selection.length) hud.push({ text: `Selection: ${selection.length}` });
    else if (target && toolId === "pushpull") hud.push({ text: `Selected: ${describeTarget(target)}` });
  } else if (kernelState === "loading") {
    hud.push({ text: "Kernel loading…" });
  } else {
    hud.push({ text: kernelNote ?? `Kernel error: ${message?.text ?? "unavailable"}`, tone: "err" });
  }
  hud.push({ text: `Units: ${u.label}` });
  hud.push({ text: `Edit: ${contextTrail(graphOf(st))} · undo ${st?.undo_depth ?? 0} · redo ${st?.redo_depth ?? 0}` });

  // Push targets show as a selection while the Push tool is active.
  const highlightEntities = useMemo<Entity[]>(() => {
    if (selection.length || toolId !== "pushpull" || !target) return selection;
    return [target.kind === "face" ? { kind: "face", object: target.object, face: target.face } : { kind: "region", sketch: target.sketch, region: target.region }];
  }, [selection, toolId, target]);
  // A part hovered in a nesting diagram lights up in the model.
  const highlight = useMemo(() => highlightFor(highlightEntities, nestHover ?? hover, scene?.mesh ?? null, scene?.state.graph ?? null), [highlightEntities, nestHover, hover, scene]);

  // Guides and dimensions draw under the tool's own overlay.
  const annotations = useMemo(() => annotationView(st ?? null, fmt, selection), [st, fmt, selection]);
  // The open group or component: a dashed box around what is in it (SketchUp-like).
  const contextBox = useMemo(() => contextBoxOverlay(st ?? null), [st]);
  // Grain arrows on every drawn piece while the Cut list pane is open.
  const grain = useMemo(() => (cutOpen ? grainOverlay(cutList) : []), [cutOpen, cutList]);
  const overlay = useMemo(() => [...contextBox, ...grain, ...annotations.overlay, ...toolView.overlay], [contextBox, grain, annotations, toolView.overlay]);
  const labels = useMemo(() => [...annotations.labels, ...toolView.labels], [annotations, toolView.labels]);

  /** Switch one of the style's section settings (one undo step). */
  const setSectionStyle = useCallback(
    (next: Partial<FullStyle>) => {
      const s = fullStyle(live.current.scene?.state.style);
      const style = compactStyle({ ...s, preset: null, ...next });
      const what =
        next.section_planes !== undefined
          ? `Section planes ${next.section_planes ? "shown" : "hidden"}`
          : next.section_cuts !== undefined
            ? `Section cuts ${next.section_cuts ? "on" : "off"}`
            : `Section fill ${next.section_fill ? "on" : "off"}`;
      void apply({ op: "set_style", style }, () => `${what}.`);
    },
    [apply],
  );

  const menus = useMemo<Record<string, MenuItem[]>>(
    () => ({
      File: [
        { label: "New", onSelect: () => void newDocument() },
        { label: "Open…", keys: "Ctrl+O", onSelect: () => void openDialog() },
        { label: "Save", keys: "Ctrl+S", onSelect: () => void save(false) },
        { label: "Save As…", keys: "Ctrl+Shift+S", onSelect: () => void save(true) },
        "-",
        { label: "Import…", keys: "Ctrl+I", title: "SketchUp, STL, 3MF, glTF, OBJ, DXF or SVG, as one undo step", onSelect: () => void importDialog() },
        {
          label: "Import units",
          items: IMPORT_UNITS.map((x) => ({
            label: `${x.label} (STL, OBJ, unitless DXF)`,
            checked: importUnits === x.id,
            onSelect: () => {
              setImportUnits(x.id);
              try {
                localStorage.setItem(IMPORT_UNITS_KEY, x.id);
              } catch {
                /* per-viewer convenience only */
              }
            },
          })),
        },
        {
          label: "Export",
          items: [
            ...EXPORTS.map((c) => ({ label: c.label, title: c.hint, onSelect: () => void exportAs(c) })),
            "-" as const,
            { label: "Section slice (SVG)…", disabled: !st?.graph?.sections?.length, title: "The selected (or active) section plane's cut, filled, 1:1 in millimetres", onSelect: () => void exportSection(null, "svg-section") },
            { label: "Section slice (DXF)…", disabled: !st?.graph?.sections?.length, title: "The selected (or active) section plane's cut as closed polylines, 1:1 in millimetres", onSelect: () => void exportSection(null, "dxf-section") },
            "-" as const,
            { label: "Sheets (PDF)…", disabled: !scene?.sheets?.length, title: "Every drawing sheet, a page each, vector, at its paper size", onSelect: () => exportSheets("pdf-all") },
          ],
        },
        { label: "Print Sheets…", disabled: !scene?.sheets?.length, title: "Every drawing sheet through the system print dialog, at 100%", onSelect: () => exportSheets("print-all") },
      ],
      Edit: [
        { label: `Undo${stepName(st?.undo_depth, st?.stats.hash) ? ` ${stepName(st?.undo_depth, st?.stats.hash)}` : ""}`, keys: "Ctrl+Z", disabled: !st?.can_undo, onSelect: () => void undo() },
        { label: "Redo", keys: "Ctrl+Y", disabled: !st?.can_redo, onSelect: () => void redo() },
        "-",
        { label: "Select All", keys: "Ctrl+A", onSelect: selectAll },
        ...(selection.length === 1 && selection[0].kind === "section"
          ? (() => {
              const id = (selection[0] as { section: number }).section;
              const sec = sectionById(st?.graph, id);
              return [
                "-" as const,
                { label: sec?.active ? "Deactivate Cut" : "Activate Cut", onSelect: () => void sectionAction(sec?.active ? "deactivate" : "activate", [id]) },
                { label: "Reverse Section Plane", onSelect: () => void sectionAction("reverse", [id]) },
                { label: "Create Group from Slice", onSelect: () => void sectionAction("slice", [id]) },
                "-" as const,
              ];
            })()
          : []),
        { label: "Select None", keys: "Esc", onSelect: () => setSelection([]) },
        { label: "Delete", keys: "Delete", disabled: !selection.length, onSelect: eraseSelection },
        { label: "Delete Guides", disabled: !st?.guides?.length, onSelect: () => void apply({ op: "delete_guides" }, () => "Guides deleted.") },
        "-",
        { label: "Make Group", keys: "Ctrl+G", disabled: !itemsOf(selection).length, onSelect: () => void makeGroup(itemsOf(selection)) },
        { label: "Make Component…", disabled: !itemsOf(selection).length, onSelect: () => makeComponent(itemsOf(selection)) },
        {
          label: "Make Unique",
          disabled: !(selection.length === 1 && selection[0].kind === "node" && (st?.graph?.nodes.find((n) => n.id === (selection[0] as { node: number }).node)?.kind ?? "") === "instance"),
          onSelect: () => selection[0]?.kind === "node" && void apply({ op: "make_unique", node: selection[0].node }, () => "Made unique: this copy has its own definition now."),
        },
        { label: "Explode", keys: "Ctrl+Shift+G", disabled: !selection.some((x) => x.kind === "node"), onSelect: () => void explode(selection.flatMap((x) => (x.kind === "node" ? [x.node] : []))) },
        {
          label: "Clear Paint",
          title: "Take the paint off what the selection holds, so the group's (or the solid's container's) paint shows; one undo step",
          disabled: !itemsOf(selection).length,
          onSelect: () =>
            void apply({ op: "clear_paint", items: itemsOf(selection) }, (r) =>
              r.outcome.kind === "paint_cleared" ? `Paint cleared from ${plural(r.outcome.cleared_faces, "face")} and ${plural(r.outcome.cleared_items, "item")}: what holds them shows now.` : "Paint cleared.",
            ),
        },
        { label: "Hide", disabled: !itemsOf(selection).length, onSelect: () => void apply({ op: "set_hidden", items: itemsOf(selection), hidden: true }, () => `Hid ${plural(itemsOf(selection).length, "item")} (the Outliner's eye shows them again).`) },
        { label: "Lock", disabled: !itemsOf(selection).length, onSelect: () => void apply({ op: "set_locked", items: itemsOf(selection), locked: true }, () => `Locked ${plural(itemsOf(selection).length, "item")}.`) },
        {
          label: "Close Group / Component",
          keys: "Esc",
          disabled: (st?.graph?.context ?? null) === null,
          onSelect: () => void apply({ op: "leave_context" }, (r) => `Closed it. Editing ${contextTrail(r.scene.state.graph)}.`),
        },
        {
          label: "Delete Imported Meshes",
          title: "Imported geometry that is not a solid",
          disabled: !st?.meshes?.length,
          onSelect: () => {
            const ops: KernelOp[] = (st?.meshes ?? []).map((m) => ({ op: "delete_mesh", mesh: m.id }));
            void apply(ops.length === 1 ? ops[0] : { op: "batch", ops }, () => `Deleted ${plural(ops.length, "imported mesh")}.`);
          },
        },
        "-",
        { label: "Restore Unsaved Work", disabled: !pendingRestore || pendingRestore.state !== "ok", onSelect: () => pendingRestore && restore(pendingRestore) },
        "-",
        { label: "Settings…", title: "The render service (renders are off until one is set and answering)", onSelect: () => setSettingsOpen(true) },
      ],
      View: [
        // Lane 12: the model or its drawing sheets.
        { label: "Model", checked: mode === "model", onSelect: () => setMode("model") },
        { label: "Sheets", checked: mode === "sheets", title: "Drawing sheets: scaled views of the model on paper", onSelect: () => setMode("sheets") },
        "-",
        { label: "Zoom Extents", keys: "Shift+Z", onSelect: zoomExtents },
        "-",
        { label: "Perspective", checked: projection === "perspective", onSelect: () => setProjection((p) => (p === "perspective" ? "parallel" : "perspective")) },
        "-",
        ...VIEWS.map((v) => ({ label: `${v.label} view`, onSelect: () => setView(v.id) })),
        "-",
        { label: "Add Scene", title: "Save this view (camera, hidden tags, open group) as a scene tab", onSelect: addScene },
        "-",
        // Lane 12: the style's section switches, here as in SketchUp's View menu.
        { label: "Section Planes", checked: fullStyle(st?.style).section_planes, onSelect: () => setSectionStyle({ section_planes: !fullStyle(st?.style).section_planes }) },
        { label: "Section Cuts", checked: fullStyle(st?.style).section_cuts, onSelect: () => setSectionStyle({ section_cuts: !fullStyle(st?.style).section_cuts }) },
        { label: "Section Fill", checked: fullStyle(st?.style).section_fill, disabled: !fullStyle(st?.style).section_cuts, onSelect: () => setSectionStyle({ section_fill: !fullStyle(st?.style).section_fill }) },
        ...(st?.scenes?.length
          ? [
              "-" as const,
              ...st.scenes.map((sc) => ({
                label: `Scene: ${sc.name}`,
                checked: activeScene === sc.name,
                onSelect: () => goScene(sc.name),
              })),
            ]
          : []),
      ],
      Plugins: plugins.menu,
      Tools: [{ label: "Section Plane", title: "Cut the model with a plane (no default key)", checked: toolId === "section", onSelect: () => selectTool("section") }],
      // The browser demo's way back to the site (the desktop app has no Help entries yet).
      ...(isWebDemo()
        ? {
            Help: [
              { label: "User Guide", onSelect: () => openLink(sitePage("guide/")) },
              { label: "What's New", onSelect: () => openLink(sitePage("changelog/")) },
              "-" as const,
              { label: "Download the Desktop App", title: "Windows, macOS and Linux installers on GitHub", onSelect: () => openLink(RELEASES_URL) },
              { label: "Source on GitHub", onSelect: () => openLink(REPO_URL) },
            ],
          }
        : {}),
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [st?.can_undo, st?.can_redo, st?.guides?.length, st?.meshes, st?.scenes, st?.graph, st?.style, toolId, apply, undo, redo, zoomExtents, projection, setView, selectAll, eraseSelection, selection, pendingRestore, restore, newDocument, openDialog, save, importDialog, importUnits, exportAs, exportSection, sectionAction, makeGroup, makeComponent, explode, addScene, goScene, activeScene, plugins.menu, mode, setMode, scene?.sheets, exportSheets],
  );

  const valueLabel = active?.valueLabel() ?? null;

  // Render credits (lane 8): the chip and the Render pane share one connection and balance.
  // Lane 15: both appear only when a render service is set (Settings) and answers.
  const gate = useRenderGate();
  const renderOn = gate.status === "ready";
  const credits = useCredits(undefined, { baseUrl: gate.url, enabled: renderOn });
  const [renderFocus, setRenderFocus] = useState(0);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const capture = useCallback(async () => {
    const engine = engineRef.current;
    if (!engine) throw new NothingToRenderError("The 3D view is not available (WebGL2 is off in this browser), so there is nothing to render from.");
    return renderPasses(engine.captureSource());
  }, []);
  const cs = credits.state;
  const chipTitle =
    cs.connection === "connected"
      ? `Render credits on this device (${cs.balance?.devices ?? 1} of ${cs.balance?.max_devices ?? 2} devices). ${cs.storage ? storageNote(cs.storage) : ""}`
      : cs.connection === "unlinked"
        ? "These credits were moved to another device with a claim code."
        : (cs.problem ?? "Render service not connected.");

  return (
    <div
      className={`app${dragging ? " dropping" : ""}${mode === "sheets" ? " sheets-mode" : ""}`}
      id="app"
      onDragOver={(e) => {
        if (!e.dataTransfer.types.includes("Files")) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "copy";
        if (!dragging) setDragging(true);
      }}
      onDragLeave={(e) => {
        if (e.currentTarget === e.target || !e.currentTarget.contains(e.relatedTarget as Node)) setDragging(false);
      }}
      onDrop={(e) => {
        if (!e.dataTransfer.files.length) return;
        e.preventDefault();
        setDragging(false);
        void takeBrowserFiles([...e.dataTransfer.files]);
      }}
    >
      <input
        ref={fileInputRef}
        type="file"
        multiple
        hidden
        data-testid="file-input"
        accept={ALL_EXTENSIONS.join(",")}
        onChange={(e) => {
          const files = [...(e.currentTarget.files ?? [])];
          e.currentTarget.value = "";
          void takeBrowserFiles(files);
        }}
      />
      {dragging ? <div className="drop-hint">Drop a .shz to open it, or a SketchUp, STL, 3MF, glTF, OBJ, DXF or SVG file to import it</div> : null}
      {isWebDemo() ? <DemoBanner /> : null}
      {report ? <ImportReport report={report} onClose={() => setReport(null)} /> : null}
      {nameRequest ? <NameDialog req={nameRequest} onClose={() => setNameRequest(null)} /> : null}
      {settingsOpen ? <SettingsDialog url={gate.url} status={gate.status} onUrl={gate.setUrl} onRecheck={gate.recheck} onClose={() => setSettingsOpen(false)} /> : null}
      {plugins.dialogs}
      <CutListReport list={cutOpen ? cutList : null} title={cutTitle} />
      {stockOpen && nesting ? (
        <StockDialog
          library={nesting.library}
          materials={nestMaterials}
          rounding={nestUnits}
          units={units}
          onDefine={defineStock}
          onDelete={deleteStock}
          onDefault={setMaterialStock}
          onReset={resetStock}
          onClose={() => setStockOpen(false)}
        />
      ) : null}
      <TopBar
        view={camView}
        onView={setView}
        credits={cs.connection === "connected" ? cs.credits : null}
        showCredits={renderOn}
        creditsState={cs.connection}
        creditsTitle={chipTitle}
        onCredits={() => setRenderFocus((n) => n + 1)}
        menus={menus}
      />
      <ToolRail active={toolId} onSelect={selectTool} plugins={plugins.toolDefs} />
      <SceneTabs
        scenes={st?.scenes ?? []}
        active={activeScene}
        onGo={goScene}
        onAdd={addScene}
        onUpdate={updateScene}
        onRename={renameScene}
        onDelete={deleteScene}
        lead={<ModeSwitch mode={mode} onMode={setMode} sheets={scene?.sheets?.length ?? 0} />}
      />
      {mode === "sheets" ? (
        <SheetsWorkspace
          lead={<ModeSwitch mode={mode} onMode={setMode} sheets={scene?.sheets?.length ?? 0} />}
          sheets={scene?.sheets ?? []}
          scenes={st?.scenes ?? []}
          stateKey={`${st?.stats.hash ?? ""}:${kernelState}`}
          kernel={() => kernelRef.current}
          apply={apply}
          message={(text, error = false) => setMessage({ text, error })}
          href={sheetHref}
          texturePaths={(st?.textures ?? []).map((t) => t.path)}
          camera={sheetCamera}
          metric={units === "mm" || units === "cm" || units === "m"}
          docName={docName ? stem(docName) : "Untitled"}
        />
      ) : null}
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
        overlay={overlay}
        marker={toolView.marker}
        labels={labels}
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
          setEngineObj(e);
        }}
      />
      <SidePanel
        credits={credits}
        renderOn={renderOn}
        capture={capture}
        focusRender={renderFocus}
        focusTags={focusTags}
        onPane={setPane}
        plugins={plugins.pane}
        focusPlugins={focusPlugins}
        cutlist={{
          list: cutList,
          problem: cutProblem ?? (kernelState === "missing" || kernelState === "error" ? "The cut list is worked out by the kernel, which is not loaded." : null),
          rounding: cutPick,
          follows: followRounding(units),
          onRounding: setRounding,
          pinned: cutPinned,
          onPin: pinRounding,
          visibleOnly: cutVisibleOnly,
          onVisibleOnly: setCutVisibleOnly,
          selected: cutSelected,
          onSelectRows: selectCutRows,
          onGrain: setGrain,
          onStock: setStock,
          onInclude: setInclude,
          onGroupLoose: groupLoose,
          onOpenPlace: openLoosePlace,
          onCsv: () => void saveCut(csvBytes, "cut list.csv", CSV_TYPE, "the cut list as a spreadsheet"),
          onPdf: () => void saveCut((l) => reportPdf(l, cutTitle), "cut list.pdf", PDF_TYPE, "the cut list report"),
          onPrint: printCut,
          onLabels: () => void saveCut((l) => labelsPdf(l, cutTitle), "labels.pdf", PDF_TYPE, `${plural(cutList?.rows.length ?? 0, "label")} on Avery 5160-size sheets`),
          nestOpen,
          onNest: toggleNest,
          nesting: nestShown ? (
            <NestingSection
              nesting={nesting}
              problem={nestProblem ?? (kernelState === "missing" || kernelState === "error" ? "Nesting is worked out by the kernel, which is not loaded." : null)}
              busy={nestBusy}
              rounding={nestUnits}
              units={units}
              onSettings={setNestSettings}
              onStock={() => setStockOpen(true)}
              onHover={hoverPlacement}
              onSelect={selectPlacement}
              onSvg={(piece) => void saveNestSvg(piece)}
              onDxf={(f) => void saveNestDxf(f)}
              onPdf={() => void saveNestPdf()}
              lit={nestLit}
            />
          ) : null,
        }}
        focusMaterials={focusMaterials}
        materials={{
          state: st ?? null,
          library,
          swatch: swatchFor,
          active: activeMat,
          onActive: (a) => {
            setActiveMat(a);
            if (a) {
              selectTool("paint");
              setMessage({ text: `Paint with ${a.name}: click a face. Alt+click picks a material up.`, error: false });
            }
          },
          onDefine: (def) => void apply({ op: "define_material", ...def }, () => `Material ${quoted(def.name)} changed.`),
          onRename: (name, to) =>
            void apply({ op: "rename_material", name, to }, () => `Material ${quoted(name)} renamed to ${quoted(to)}.`).then((r) => {
              if (r && activeMatRef.current?.name === name) setActiveMat({ from: "model", name: to });
            }),
          onDelete: (name) =>
            void apply({ op: "delete_material", name }, () => `Material ${quoted(name)} deleted: what was painted with it shows what is around it now.`).then((r) => {
              if (r && activeMatRef.current?.name === name) setActiveMat(null);
            }),
          onImport: (file) => void importImage(file),
          onPosition: positionSelected,
          units,
        }}
        styles={{
          style: st?.style ?? null,
          shadows: st?.shadows ?? null,
          onStyle: (style) => void apply({ op: "set_style", style }, () => `Style: ${style?.preset ?? "custom"}.`),
          onShadows: (shadows) => void apply({ op: "set_shadows", shadows }, () => (shadows.on ? "Shadows on." : "Shadows off.")),
          onPreviewShadows: setShadowPreview,
          softenTarget: softenTargetOf(selection),
          onSoften: (angle, soft, smooth) => {
            const t = softenTargetOf(live.current.selection);
            if (!t) return;
            void apply({ op: "soften_edges", objects: t.objects, angle, soft, smooth }, (r) => {
              const n = r.outcome.kind === "edges_set" ? r.outcome.changed : 0;
              const what = soft && smooth ? "softened and smoothed" : soft ? "softened" : smooth ? "smoothed" : "made hard";
              return `${t.label}: ${n} edge${n === 1 ? "" : "s"} changed (faces turning by at most ${angle}° ${what}, the rest hard).`;
            });
          },
        }}
        outliner={{ state: st ?? null, selection, onSelect: outlinerSelect, onAction: outlinerAction, onRename: outlinerRename }}
        tags={{
          state: st ?? null,
          canTag: itemsOf(selection).length > 0,
          onCreate: () => void createTag(),
          onVisible: (tag, visible) => void apply({ op: "set_tag_visible", tag, visible }, () => `Tag ${quoted(tag)} ${visible ? "shown" : "hidden"}.`),
          onColor: (tag, color) => void apply({ op: "set_tag_color", tag, color: hexToRgb(color) }),
          onRename: (tag, name) => {
            const created = createdTagRef.current === tag;
            createdTagRef.current = null;
            void apply({ op: "rename_tag", tag, name }, () => (created ? `Tag ${quoted(name)} created.` : `Tag ${quoted(tag)} renamed to ${quoted(name)}.`));
          },
          onDelete: (tag) => void apply({ op: "delete_tag", tag }, () => `Tag ${quoted(tag)} deleted: what was on it is untagged now.`),
          onAssign: (tag) => {
            const items = itemsOf(live.current.selection);
            void apply({ op: "tag_items", items, tag }, () => `${plural(items.length, "item")} on ${tag ? `tag ${quoted(tag)}` : "Untagged"}.`);
          },
          editing: editingTag,
          onEdited: () => {
            setEditingTag(null);
            // Naming a new tag reads "created" only straight after New tag (onRename runs right
            // after this, synchronously); later renames read "renamed".
            setTimeout(() => (createdTagRef.current = null), 0);
          },
        }}
      />
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

/** Open a page of the site or GitHub in a new tab (browser demo only). */
function openLink(url: string) {
  window.open(url, "_blank", "noopener,noreferrer");
}
