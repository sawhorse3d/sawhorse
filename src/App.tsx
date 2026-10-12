import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { TopBar, type MenuItem } from "./components/TopBar";
import { ToolRail } from "./components/ToolRail";
import { Viewport, type KernelState, type ViewportPointer } from "./components/Viewport";
import { SidePanel, type PaneId } from "./components/SidePanel";
import { rowKey } from "./components/CutListPane";
import { grainOverlay } from "./cutlist/arrows";
import { defaultRounding, GRAIN_WORDS } from "./cutlist/format";
import type { CutList, CutRow, Grain, Rounding } from "./kernel/types";
import { ValueBar, type StatusMessage } from "./components/ValueBar";
import { createKernel, KernelUnavailableError, type EditResult, type KernelClient } from "./kernel/client";
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
import { autosaveEnabled, clearAutosave, describeAutosave, loadAutosave, saveAutosave, worthRestoring, type Autosave } from "./lib/autosave";
import { ALL_EXTENSIONS, IMPORT_TYPES, SHZ, extensionOf, onTauriDrop, pickFile, readBrowserFile, saveFile, stem, type FileHandle, type PickedFile } from "./io/files";
import { EXPORTS, MTL_TYPE, type ExportChoice } from "./io/exports";
import type { ExportOptions, ImportUnit, OpOutcome } from "./kernel/types";
import { ImportReport, type Report } from "./components/ImportReport";
import { NameDialog, type NameRequest } from "./components/NameDialog";
import { SceneTabs } from "./components/SceneTabs";
import type { OutlineRow, OutlinerAction } from "./components/Outliner";
import { TAG_PALETTE, hexToRgb } from "./components/TagsPane";
import { applySelection } from "./lib/entities";
import type { Item, SceneDef } from "./kernel/types";
import { useCredits } from "./credits/useCredits";
import { storageNote } from "./credits/device";
import { NothingToRenderError, renderPasses } from "./render/passes";

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
/** The cut list's rounding when someone picked one (otherwise it follows the units). */
const CUT_ROUNDING_KEY = "sawhorse.cutRounding";

function loadCutRounding(): Rounding | null {
  try {
    const v = localStorage.getItem(CUT_ROUNDING_KEY);
    if (v === "sixteenth" || v === "thirty_second" || v === "half_mm") return v;
  } catch {
    /* storage blocked */
  }
  return null;
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

const plural = (n: number, w: string) => `${n} ${w}${n === 1 ? "" : "s"}`;

/** One line for the status bar after an import or open. */
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

function isTextEntry(el: EventTarget | null): boolean {
  if (!(el instanceof HTMLElement)) return false;
  return el.isContentEditable || el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement;
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
  const cutOpen = pane === "cutlist";
  const [cutPick, setCutPick] = useState<Rounding | null>(loadCutRounding);
  const cutRounding = cutPick ?? defaultRounding(units);
  const [cutVisibleOnly, setCutVisibleOnly] = useState(false);
  const [cutList, setCutList] = useState<CutList | null>(null);
  const [cutProblem, setCutProblem] = useState<string | null>(null);
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
  const scheduleAutosave = useCallback(() => {
    if (!autosaveEnabled()) return;
    if (autosaveTimer.current) clearTimeout(autosaveTimer.current);
    autosaveTimer.current = setTimeout(() => {
      const k = kernelRef.current;
      if (!k) return;
      void k
        .log()
        .then((log) => saveAutosave({ log, savedAt: Date.now(), solids: live.current.scene?.state.stats.objects ?? 0 }))
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
  const undo = useCallback(
    () =>
      run((k) => k.undo(), null, (r) => `Undone. ${r.scene.state.undo_depth} more step(s) to undo.`).then((r) => {
        if (r) afterHistoryRef.current();
        return r;
      }),
    [run],
  );
  const redo = useCallback(
    () =>
      run((k) => k.redo(), null, (r) => `Redone. ${r.scene.state.redo_depth} more step(s) to redo.`).then((r) => {
        if (r) afterHistoryRef.current();
        return r;
      }),
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
          const s = await k.replay(a.log);
          live.current.scene = s;
          setNameSource(s.state, s.mesh);
          setScene(s);
          live.current.target = null;
          setTarget(null);
          live.current.selection = [];
          setSelection([]);
          setMessage({ text: `Restored your unsaved work: ${s.state.stats.objects} solid${s.state.stats.objects === 1 ? "" : "s"}, ${a.log.length} steps (undo works as before).`, error: false });
          setFitSeq((n) => n + 1);
          scheduleAutosave();
        } catch (err) {
          setMessage({ text: `Could not restore the unsaved work: ${friendlyError(err instanceof Error ? err.message : String(err))}`, error: true });
        }
      });
      void next.finally(() => editsPending.current--);
      queue.current = next;
    },
    [scheduleAutosave],
  );
  const discardRestore = useCallback(() => {
    setPendingRestore(null);
    void clearAutosave();
    setMessage({ text: "Unsaved work discarded.", error: false });
  }, []);

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
      setMessage({ text: `Importing ${f.name}…`, error: false });
      let notes: string[] = [];
      const r = await run(
        async (k) => {
          const res = ext === ".skp" ? await k.importSkp(f.name, f.bytes) : await k.importFile(f.name, f.bytes, { units: importUnits });
          notes = res.notes;
          return res;
        },
        null,
        (res) => describeImport(f.name, res.outcome, res.scene.state.graph),
      );
      if (!r) {
        setReport({ title: `Could not import ${f.name}`, lines: [live.current.lastError ?? "refused"], error: true });
        return;
      }
      const kernelNotes = r.outcome.kind === "imported" ? r.outcome.notes : [];
      setReport({ title: describeImport(f.name, r.outcome, r.scene.state.graph), lines: [...notes, ...kernelNotes] });
      const w = window as unknown as { __sawhorse?: Record<string, unknown> };
      w.__sawhorse = { ...(w.__sawhorse ?? {}), lastImport: { name: f.name, outcome: r.outcome, notes: [...notes, ...kernelNotes] } };
      setFitSeq((n) => n + 1);
    },
    [run, importUnits],
  );

  /** Open a .shz in place of the current model. */
  const openBytes = useCallback(
    async (f: PickedFile) => {
      if (!confirmDiscard()) return;
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
      const opened = info as { notes: string[]; opened: { display_units: string | null } };
      setDocName(f.name);
      fileHandle.current = f.handle;
      setSavedLogLen(r.scene.state.log_len);
      const du = opened.opened.display_units;
      if (du && UNIT_SYSTEMS.some((x) => x.id === du)) {
        setUnits(du as UnitSystem);
        live.current.units = du as UnitSystem;
      }
      setReport(opened.notes.length ? { title: `Opened ${f.name}`, lines: opened.notes } : null);
      setSelection([]);
      setFitSeq((n) => n + 1);
    },
    [run, confirmDiscard],
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

  const newDocument = useCallback(async () => {
    if (!confirmDiscard()) return;
    const r = await run((k) => k.newDocument(), null, () => "New model.");
    if (r) {
      setDocName(null);
      fileHandle.current = undefined;
      setSavedLogLen(r.scene.state.log_len);
      setReport(null);
      setSelection([]);
      setView("iso");
    }
  }, [run, confirmDiscard]);

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
        const png = (await engineRef.current?.snapshotPng(512)) ?? new Uint8Array();
        const bytes = await k.saveShz(live.current.units, png);
        const name = `${docName ? stem(docName) : "Untitled"}.shz`;
        const res = await saveFile(name, bytes, SHZ, as ? undefined : fileHandle.current);
        if (!res) return;
        setDocName(res.name);
        fileHandle.current = res.handle;
        setSavedLogLen(live.current.scene?.state.log_len ?? null);
        setMessage({ text: `Saved ${res.name} (${(bytes.length / 1024).toFixed(0)} KB).`, error: false });
      } catch (e) {
        setMessage({ text: `Could not save: ${e instanceof Error ? e.message : String(e)}`, error: true });
      }
    },
    [docName],
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
    void onTauriDrop((files) => void takeFiles(files)).then((f) => (off = f));
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
    k.init()
      .then(async (s) => {
        setNameSource(s.state, s.mesh);
        live.current.scene = s;
        setScene(s);
        setKernelState("ready");
        setSavedLogLen(s.state.log_len);
        if (!autosaveEnabled()) return;
        const saved = await loadAutosave();
        if (!worthRestoring(saved)) return;
        setPendingRestore(saved);
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
    setMessage({
      text: `Restore unsaved work? ${describeAutosave(pendingRestore)}.`,
      error: false,
      actions: [
        { label: "Restore", run: () => restore(pendingRestore) },
        { label: "Discard", run: discardRestore },
      ],
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
        setCutList(l);
        setCutProblem(null);
      })
      .catch((e) => !stale && setCutProblem(`The cut list could not be worked out: ${friendlyError(e instanceof Error ? e.message : String(e))}`));
    return () => {
      stale = true;
    };
  }, [cutOpen, scene, cutRounding, cutVisibleOnly]);

  // Read-only hook for tests: the cut list for any options, straight from the kernel.
  useEffect(() => {
    const w = window as unknown as { __sawhorse?: Record<string, unknown> };
    w.__sawhorse = {
      ...(w.__sawhorse ?? {}),
      cutList: async (o?: { rounding?: Rounding; visible_only?: boolean }) => {
        await queue.current;
        return kernelRef.current?.cutList({ rounding: o?.rounding ?? "sixteenth", visible_only: !!o?.visible_only }) ?? null;
      },
    };
  }, []);

  const setRounding = useCallback((r: Rounding) => {
    setCutPick(r);
    try {
      localStorage.setItem(CUT_ROUNDING_KEY, r);
    } catch {
      /* per-viewer convenience only */
    }
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
    }),
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
      const hint = next?.variantHint();
      if (hint) setMessage({ text: hint, error: false });
      setToolView(next?.view() ?? EMPTY_VIEW);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [defaultValue],
  );

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

  const outlinerAction = useCallback(
    (action: OutlinerAction, rows: OutlineRow[]) => {
      const entities = rows.flatMap((r) => (r.entity ? [r.entity] : []));
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

  /** A cut list row clicked: select its drawn pieces, opening what holds them when they share
   * a closed group or component (as the Outliner does); pieces in several places select what
   * holds each of them in the open context. */
  const selectCutRow = useCallback(
    (row: CutRow, mode: "replace" | "add" | "toggle") => {
      const state = live.current.scene?.state;
      const g = graphOf(state);
      const pieces: Entity[] = row.occurrences.filter((o) => o.visible).map((o) => ("node" in o.item ? { kind: "node", node: o.item.node } : { kind: "object", object: o.item.object }));
      if (!pieces.length) return void setMessage({ text: `Every '${row.name}' is hidden: show it (the Outliner's eye or its tag) to select it.`, error: true });
      const parentOf = (e: Entity) => (e.kind === "node" ? (g.nodes.find((n) => n.id === e.node)?.parent ?? null) : e.kind === "object" ? (g.objects.find((o) => o.id === e.object)?.parent ?? null) : null);
      const parents = new Set(pieces.map(parentOf));
      const what = `${plural(pieces.length, "piece")} of ${quoted(row.name)}`;
      if (parents.size === 1) {
        const p = [...parents][0];
        if (p === g.context) {
          select(applySelection(live.current.selection, pieces, mode));
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
      select(applySelection(live.current.selection, holders, mode));
      setMessage({ text: `The ${what} are in ${holders.length === 1 ? "one group or component" : `${holders.length} groups or components`}; selected ${holders.length === 1 ? "it" : "those"}.`, error: false });
    },
    [apply, select],
  );

  const cutSelected = useMemo(() => {
    const keys = new Set(selection.map(entityKey));
    const out = new Set<string>();
    for (const row of cutList?.rows ?? []) {
      const vis = row.occurrences.filter((o) => o.visible);
      if (vis.length && vis.every((o) => keys.has(entityKey("node" in o.item ? { kind: "node", node: o.item.node } : { kind: "object", object: o.item.object })))) out.add(rowKey(row));
    }
    return out;
  }, [selection, cutList]);

  const setGrain = useCallback(
    (row: CutRow, grain: Grain) => void apply({ op: "set_grain", part: row.target, grain }, () => `Grain of ${quoted(row.name)}: ${GRAIN_WORDS[grain]}.`),
    [apply],
  );
  const setStock = useCallback(
    (row: CutRow, stock: string | null, sheet: boolean | null) =>
      void apply({ op: "set_stock", part: row.target, stock, sheet }, () =>
        stock ? `${quoted(row.name)} is cut from ${stock}${sheet === null ? "" : sheet ? " (sheet goods)" : " (solid lumber)"}.` : sheet === null ? `${quoted(row.name)} uses its material's name again.` : `${quoted(row.name)} counted as ${sheet ? "sheet goods" : "solid lumber"}.`,
      ),
    [apply],
  );

  // CSV, PDF, Print and Labels arrive with the exports (the next step of this lane).
  const exportsSoon = useCallback(() => setMessage({ text: "Cut list exports arrive with the next step.", error: false }), []);

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
      const textEntry = isTextEntry(e.target) && !(inVcb && !live.current.vcbEditing);
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
  const highlight = useMemo(() => highlightFor(highlightEntities, hover, scene?.mesh ?? null, scene?.state.graph ?? null), [highlightEntities, hover, scene]);

  // Guides and dimensions draw under the tool's own overlay.
  const annotations = useMemo(() => annotationView(st ?? null, fmt, selection), [st, fmt, selection]);
  // The open group or component: a dashed box around what is in it (SketchUp-like).
  const contextBox = useMemo(() => contextBoxOverlay(st ?? null), [st]);
  // Grain arrows on every drawn piece while the Cut list pane is open.
  const grain = useMemo(() => (cutOpen ? grainOverlay(cutList) : []), [cutOpen, cutList]);
  const overlay = useMemo(() => [...contextBox, ...grain, ...annotations.overlay, ...toolView.overlay], [contextBox, grain, annotations, toolView.overlay]);
  const labels = useMemo(() => [...annotations.labels, ...toolView.labels], [annotations, toolView.labels]);

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
          items: EXPORTS.map((c) => ({ label: c.label, title: c.hint, onSelect: () => void exportAs(c) })),
        },
      ],
      Edit: [
        { label: "Undo", keys: "Ctrl+Z", disabled: !st?.can_undo, onSelect: () => void undo() },
        { label: "Redo", keys: "Ctrl+Y", disabled: !st?.can_redo, onSelect: () => void redo() },
        "-",
        { label: "Select All", keys: "Ctrl+A", onSelect: selectAll },
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
        { label: "Restore Unsaved Work", disabled: !pendingRestore, onSelect: () => pendingRestore && restore(pendingRestore) },
      ],
      View: [
        { label: "Zoom Extents", keys: "Shift+Z", onSelect: zoomExtents },
        "-",
        { label: "Perspective", checked: projection === "perspective", onSelect: () => setProjection((p) => (p === "perspective" ? "parallel" : "perspective")) },
        "-",
        ...VIEWS.map((v) => ({ label: `${v.label} view`, onSelect: () => setView(v.id) })),
        "-",
        { label: "Add Scene", title: "Save this view (camera, hidden tags, open group) as a scene tab", onSelect: addScene },
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
    }),
    [st?.can_undo, st?.can_redo, st?.guides?.length, st?.meshes, st?.scenes, st?.graph, apply, undo, redo, zoomExtents, projection, setView, selectAll, eraseSelection, selection, pendingRestore, restore, newDocument, openDialog, save, importDialog, importUnits, exportAs, makeGroup, makeComponent, explode, addScene, goScene, activeScene],
  );

  const valueLabel = active?.valueLabel() ?? null;

  // Render credits (lane 8): the chip and the Render pane share one connection and balance.
  const credits = useCredits();
  const [renderFocus, setRenderFocus] = useState(0);
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
      className={`app${dragging ? " dropping" : ""}`}
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
        const files = [...e.dataTransfer.files];
        void Promise.all(files.map(readBrowserFile)).then(takeFiles);
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
          void Promise.all(files.map(readBrowserFile)).then(takeFiles);
        }}
      />
      {dragging ? <div className="drop-hint">Drop a .shz to open it, or a SketchUp, STL, 3MF, glTF, OBJ, DXF or SVG file to import it</div> : null}
      {report ? <ImportReport report={report} onClose={() => setReport(null)} /> : null}
      {nameRequest ? <NameDialog req={nameRequest} onClose={() => setNameRequest(null)} /> : null}
      <TopBar
        view={camView}
        onView={setView}
        credits={cs.connection === "connected" ? cs.credits : null}
        creditsState={cs.connection}
        creditsTitle={chipTitle}
        onCredits={() => setRenderFocus((n) => n + 1)}
        menus={menus}
      />
      <ToolRail active={toolId} onSelect={selectTool} />
      <SceneTabs scenes={st?.scenes ?? []} active={activeScene} onGo={goScene} onAdd={addScene} onUpdate={updateScene} onRename={renameScene} onDelete={deleteScene} />
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
        }}
      />
      <SidePanel
        credits={credits}
        capture={capture}
        focusRender={renderFocus}
        focusTags={focusTags}
        onPane={setPane}
        cutlist={{
          list: cutList,
          problem: cutProblem ?? (kernelState === "missing" || kernelState === "error" ? "The cut list is worked out by the kernel, which is not loaded." : null),
          rounding: cutRounding,
          onRounding: setRounding,
          visibleOnly: cutVisibleOnly,
          onVisibleOnly: setCutVisibleOnly,
          selected: cutSelected,
          onSelectRow: selectCutRow,
          onGrain: setGrain,
          onStock: setStock,
          onCsv: exportsSoon,
          onPdf: exportsSoon,
          onPrint: exportsSoon,
          onLabels: exportsSoon,
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
