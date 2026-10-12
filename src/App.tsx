import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { TopBar, type MenuItem } from "./components/TopBar";
import { ToolRail } from "./components/ToolRail";
import { Viewport, type KernelState, type ViewportPointer } from "./components/Viewport";
import { SidePanel } from "./components/SidePanel";
import { ValueBar, type StatusMessage } from "./components/ValueBar";
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
import { useCredits } from "./credits/useCredits";
import { storageNote } from "./credits/device";
import { NothingToRenderError, renderPasses } from "./render/passes";

/** What the Value Control Box offers before anything is typed: 12 in up, 24 x 16 in rectangles. */
export const DEFAULT_DISTANCE = 12 * M_PER_IN;
export const DEFAULT_RECT: [number, number] = [24 * M_PER_IN, 16 * M_PER_IN];

const UNITS_KEY = "sawhorse.units";
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
export function describeImport(name: string, o: OpOutcome): string {
  if (o.kind === "restored") return `Opened ${name}: ${plural(o.objects, "solid")}${o.meshes ? `, ${plural(o.meshes, "mesh")} (not solid)` : ""}.`;
  if (o.kind !== "imported") return `Imported ${name}.`;
  const parts = [plural(o.objects.length, "solid")];
  if (o.meshes.length) parts.push(`${plural(o.meshes.length, "mesh")} that ${o.meshes.length === 1 ? "is" : "are"} not solid`);
  if (o.groups.length) parts.push(plural(o.groups.length, "group"));
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
  const [message, setMessage] = useState<Message>(null);
  const [selection, setSelection] = useState<Entity[]>([]);
  const [hover, setHover] = useState<Entity | null>(null);
  const [target, setTarget] = useState<Target | null>(null);
  const [groupContext, setGroupContext] = useState<number | null>(null);
  const [toolView, setToolView] = useState<ToolView>(EMPTY_VIEW);
  // The open document: its name, where it lives (for Save), the log length when last saved.
  const [docName, setDocName] = useState<string | null>(null);
  const fileHandle = useRef<FileHandle | undefined>(undefined);
  const [savedLogLen, setSavedLogLen] = useState<number | null>(null);
  const [report, setReport] = useState<Report | null>(null);
  const [importUnits, setImportUnits] = useState<ImportUnit>(loadImportUnits);
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
  const live = useRef({ scene, units, selection, target, groupContext, vcbEditing: false, vcbFor: null as ToolId | null, lastError: null as string | null });
  // Copy a React state value into `live` only when it changed since the last render. The
  // handlers write `live` first (imperatively) and React state after; a render that runs
  // before React has applied those updates (a keystroke renders urgently and skips the
  // edit's pending updates) must not put the stale values back: that lost the Push target
  // between "draw a rectangle" and "type a distance" when the keys came fast.
  const seen = useRef({ scene, units, selection, target, groupContext });
  {
    const prev = seen.current;
    const next = { scene, units, selection, target, groupContext };
    if (prev.scene !== scene) live.current.scene = scene;
    if (prev.units !== units) live.current.units = units;
    if (prev.selection !== selection) live.current.selection = selection;
    if (prev.target !== target) live.current.target = target;
    if (prev.groupContext !== groupContext) live.current.groupContext = groupContext;
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
          // Drop selected entities that no longer exist (faces and regions by id).
          const sel = live.current.selection.filter((e) => (e.kind === "face" || e.kind === "region" || e.kind === "object" ? partsOf(e, r.scene.mesh).length > 0 : true));
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
  const undo = useCallback(() => run((k) => k.undo(), null, (r) => `Undone. ${r.scene.state.undo_depth} more step(s) to undo.`), [run]);
  const redo = useCallback(() => run((k) => k.redo(), null, (r) => `Redone. ${r.scene.state.redo_depth} more step(s) to redo.`), [run]);

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
        (res) => describeImport(f.name, res.outcome),
      );
      if (!r) {
        setReport({ title: `Could not import ${f.name}`, lines: [live.current.lastError ?? "refused"], error: true });
        return;
      }
      const kernelNotes = r.outcome.kind === "imported" ? r.outcome.notes : [];
      setReport({ title: describeImport(f.name, r.outcome), lines: [...notes, ...kernelNotes] });
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
        (res) => describeImport(f.name, res.outcome),
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

  /** What an export takes: the selected solids, or everything visible. */
  const exportScope = useCallback((): ExportOptions["scope"] => {
    const objs = new Set<number>();
    for (const e of live.current.selection) {
      if (e.kind === "object") objs.add(e.object);
      if (e.kind === "face") objs.add(e.object);
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

  // Read-only hooks for e2e tests: every solid with its display number, bounds and volume.
  useEffect(() => {
    const w = window as unknown as { __sawhorse?: Record<string, unknown> };
    w.__sawhorse = { ...(w.__sawhorse ?? {}), solids: () => live.current.scene?.state.solids ?? [] };
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
      context: () => live.current.groupContext,
      enterGroup: (g) => {
        live.current.groupContext = g;
        setGroupContext(g);
        setSelection([]);
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
  }, [selectTool, undo, redo, zoomExtents, eraseSelection, selectAll, afterPointer, save, openDialog, importDialog]);

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
  const groupName = groupContext !== null ? st?.groups.find((g) => g.id === groupContext)?.name : null;
  hud.push({ text: `Edit: ${groupName ? `Group ${groupName}` : "Model"} · undo ${st?.undo_depth ?? 0} · redo ${st?.redo_depth ?? 0}` });

  // Push targets show as a selection while the Push tool is active.
  const highlightEntities = useMemo<Entity[]>(() => {
    if (selection.length || toolId !== "pushpull" || !target) return selection;
    return [target.kind === "face" ? { kind: "face", object: target.object, face: target.face } : { kind: "region", sketch: target.sketch, region: target.region }];
  }, [selection, toolId, target]);
  const highlight = useMemo(() => highlightFor(highlightEntities, hover, scene?.mesh ?? null), [highlightEntities, hover, scene]);

  // Guides and dimensions draw under the tool's own overlay.
  const annotations = useMemo(() => annotationView(st ?? null, fmt, selection), [st, fmt, selection]);
  const overlay = useMemo(() => [...annotations.overlay, ...toolView.overlay], [annotations, toolView.overlay]);
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
        ...(st?.scenes?.length
          ? [
              "-" as const,
              ...st.scenes.map((sc) => ({
                label: `Scene: ${sc.name}`,
                onSelect: () => {
                  const p: Projection = sc.parallel ? "parallel" : "perspective";
                  engineRef.current?.lookAt(sc.eye, sc.target, sc.up, p, sc.fov ?? 35);
                  setProjection(p);
                  setCamView(null);
                  if (sc.hidden_tags?.length) setMessage({ text: `Scene ${sc.name} hides tags: ${sc.hidden_tags.join(", ")}.`, error: false });
                },
              })),
            ]
          : []),
      ],
    }),
    [st?.can_undo, st?.can_redo, st?.guides?.length, st?.meshes, st?.scenes, apply, undo, redo, zoomExtents, projection, setView, selectAll, eraseSelection, selection.length, pendingRestore, restore, newDocument, openDialog, save, importDialog, importUnits, exportAs],
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
      <SidePanel credits={credits} capture={capture} focusRender={renderFocus} />
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
