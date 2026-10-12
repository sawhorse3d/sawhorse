// Sheets mode (lane 12, ADR-0010): the sheet tabs, the sheet tools, the sheet canvas and
// the Sheets pane, in the grid areas the model's scene tabs, tool rail, viewport and side
// panel use. The 3D view stays loaded underneath (hidden), so going back to the model is
// instant and "Use 3D view" can read its camera.
//
// Every change is one kernel op (one undo step; Ctrl+Z works here as in the model); the
// sheet on screen is the kernel's layout of it, asked for again after every edit.
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { EditResult, KernelClient } from "../kernel/client";
import type { CameraSpec, DimKind, ItemKind, Scale, SheetRow, SheetSetup, ViewportSource } from "../kernel/sheetTypes";
import type { KernelOp, SceneDef } from "../kernel/types";
import { saveFile } from "../io/files";
import { sniffImage, texturePathFor, toBase64 } from "../lib/looks";
import { DEFAULT_COLUMNS, IN, ITEM_NAMES, canBeToScale, captionFor, fittingScale, newSetup, scaleLabel, sourceLabel } from "./paper";
import { sheetSvg, type Href, type SheetRender } from "./render";
import { SheetCanvas, type PlaceRequest, type SheetCanvasHandle, type SheetToolId } from "./SheetCanvas";
import { SheetRail } from "./SheetRail";
import { SheetsPane, setupOf, type SheetExportKind } from "./SheetsPane";
import { SheetTabs } from "./SheetTabs";

const ACTIVE_KEY = "sawhorse.sheet";
const SNAP_KEY = "sawhorse.sheetSnap";
const SVG_FILE = { description: "SVG drawing", mime: "image/svg+xml", extensions: [".svg"] };

function load(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
function keep(key: string, v: string) {
  try {
    localStorage.setItem(key, v);
  } catch {
    /* a per-viewer convenience only */
  }
}

/** "Sheet 2", the first free one. */
export function nextSheetName(taken: string[]): string {
  for (let k = taken.length + 1; ; k++) if (!taken.includes(`Sheet ${k}`)) return `Sheet ${k}`;
}

/** The item as an op wants it (no id). */
function bare(k: ItemKind & { id?: number }): ItemKind {
  const { id: _id, ...rest } = k;
  void _id;
  return rest as ItemKind;
}

export interface SheetsWorkspaceProps {
  /** The Model / Sheets switch, at the start of the tab row. */
  lead: ReactNode;
  sheets: SheetRow[];
  scenes: SceneDef[];
  /** Changes with every edit (the model's hash): the sheet is laid out again. */
  stateKey: string;
  kernel: () => KernelClient | null;
  apply: (op: KernelOp, describe?: (r: EditResult) => string) => Promise<EditResult | null>;
  message: (text: string, error?: boolean) => void;
  /** An image the model holds, as something an <image> shows. */
  href: Href;
  /** Image paths the model holds (a new image gets a free one). */
  texturePaths: string[];
  /** The 3D view's camera now. */
  camera: () => CameraSpec | null;
  metric: boolean;
  docName: string;
}

export function SheetsWorkspace(props: SheetsWorkspaceProps) {
  const { sheets, scenes, stateKey, kernel, apply, message, href } = props;
  const [active, setActiveState] = useState<number | null>(() => {
    const v = Number(load(ACTIVE_KEY));
    return Number.isFinite(v) && v > 0 ? v : null;
  });
  const shown = sheets.find((s) => s.id === active) ?? sheets[0] ?? null;
  const activeId = shown?.id ?? null;
  const setActive = (id: number) => {
    setActiveState(id);
    keep(ACTIVE_KEY, String(id));
  };
  const [render, setRender] = useState<SheetRender | null>(null);
  const [selection, setSelection] = useState<number[]>([]);
  const [tool, setToolState] = useState<SheetToolId>("select");
  const [dimKind, setDimKind] = useState<DimKind>("horizontal");
  const [newSource, setNewSource] = useState<ViewportSource>(() => {
    const s = scenes.find((x) => x.parallel) ?? scenes[0];
    return s ? { from: "scene", name: s.name } : { from: "standard", view: "front" };
  });
  const [gridSnap, setGridSnap] = useState(() => load(SNAP_KEY) !== "off");
  const canvas = useRef<SheetCanvasHandle>(null);
  const imageInput = useRef<HTMLInputElement>(null);
  const pendingImage = useRef<[number, number, number, number] | null>(null);
  const live = useRef({ render, selection, activeId, tool });
  live.current = { render, selection, activeId, tool };

  const setTool = (t: SheetToolId) => {
    setToolState(t);
    if (t !== "select") setSelection([]);
  };

  // The newSource scene may be renamed or deleted.
  useEffect(() => {
    if (newSource.from === "scene" && !scenes.some((s) => s.name === newSource.name)) setNewSource({ from: "standard", view: "front" });
  }, [scenes, newSource]);

  // Lay the sheet out again after every edit (and when another sheet is shown).
  const seq = useRef(0);
  useEffect(() => {
    const k = kernel();
    if (!k || activeId === null) {
      setRender(null);
      return;
    }
    const n = ++seq.current;
    k.sheetRender(activeId)
      .then((r) => {
        if (n === seq.current) setRender(r);
      })
      .catch((e: unknown) => {
        if (n !== seq.current) return;
        setRender(null);
        message(`The sheet could not be laid out: ${e instanceof Error ? e.message : String(e)}`, true);
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeId, stateKey, sheets.length]);

  // What no longer exists leaves the selection.
  useEffect(() => {
    if (!render) return;
    setSelection((sel) => {
      const next = sel.filter((id) => render.items.some((i) => i.id === id));
      return next.length === sel.length ? sel : next;
    });
  }, [render]);

  // ---- edits --------------------------------------------------------------------------
  const addSheet = useCallback(async () => {
    const name = nextSheetName(sheets.map((s) => s.sheet.name));
    const r = await apply({ op: "add_sheet", setup: newSetup(name, props.metric) }, () => `${name} added: ${props.metric ? "A4" : "Letter"} landscape with a title block. Place a viewport with the View tool.`);
    if (r?.outcome.kind === "sheet_added") {
      setActive(r.outcome.sheet);
      setSelection([]);
      setToolState("viewport");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sheets, apply, props.metric]);

  const setSetup = (id: number, setup: SheetSetup) => void apply({ op: "set_sheet", sheet: id, setup }, () => `${setup.name} changed.`);
  const sheetName = (id: number) => sheets.find((s) => s.id === id)?.sheet.name ?? "The sheet";

  const addItem = useCallback(
    async (kind: ItemKind, describe: string, then: "select" | "keep" = "select") => {
      if (activeId === null) return;
      const r = await apply({ op: "add_sheet_item", sheet: activeId, item: kind }, () => describe);
      const out = r?.outcome;
      if (out?.kind === "sheet_item_added") {
        if (then === "select") {
          setToolState("select");
          setSelection([out.item]);
        }
      }
    },
    [activeId, apply],
  );

  const setItem = useCallback(
    (id: number, kind: ItemKind, describe?: string) => {
      if (activeId === null) return;
      void apply({ op: "set_sheet_item", sheet: activeId, item: id, kind: bare(kind) }, () => describe ?? `${ITEM_NAMES[kind.kind]} changed.`);
    },
    [activeId, apply],
  );

  const deleteItems = useCallback(
    (ids: number[]) => {
      if (activeId === null || !ids.length) return;
      void apply({ op: "delete_sheet_items", sheet: activeId, items: ids }, () => `Deleted ${ids.length === 1 ? "1 item" : `${ids.length} items`} (a viewport takes its dimensions with it).`);
      setSelection([]);
    },
    [activeId, apply],
  );

  const moveItems = useCallback(
    (ids: number[], by: [number, number]) => {
      if (activeId === null) return;
      void apply({ op: "move_sheet_items", sheet: activeId, items: ids, by }, () => `Moved ${ids.length === 1 ? "1 item" : `${ids.length} items`}.`);
    },
    [activeId, apply],
  );

  /** The largest standard scale a viewport's drawing fits at in `rect`. */
  const fitScale = async (kind: ItemKind): Promise<Scale | null> => {
    const k = kernel();
    if (!k || kind.kind !== "viewport" || !canBeToScale(kind.source, scenes)) return null;
    const f = await k.viewportFit(bare(kind));
    if (f.empty || !(f.factor > 0)) return null;
    return fittingScale(f.factor);
  };

  const place = async (req: PlaceRequest) => {
    if (activeId === null) return;
    switch (req.kind) {
      case "box": {
        const rect = req.rect;
        if (req.tool === "viewport") {
          const source = newSource;
          let scale: Scale = { kind: "nts" };
          try {
            scale = (await fitScale({ kind: "viewport", rect, source, scale })) ?? scale;
          } catch (e) {
            message(`The view could not be drawn: ${e instanceof Error ? e.message : String(e)}`, true);
            return;
          }
          await addItem({ kind: "viewport", rect, source, scale, caption: captionFor(source, scale) }, `Viewport: ${sourceLabel(source)} at ${scaleLabel(scale)}. Drag it to move it, its corners to resize; the Sheet pane changes its scale.`);
        } else if (req.tool === "table") {
          await addItem({ kind: "table", rect, columns: DEFAULT_COLUMNS, title: "CUT LIST" }, "Cut-list table placed: it follows the model.");
        } else if (req.tool === "rect") {
          await addItem({ kind: "rect", rect, width: 0.35e-3 }, "Rectangle drawn.");
        } else {
          pendingImage.current = rect;
          imageInput.current?.click();
        }
        return;
      }
      case "line":
        await addItem({ kind: "line", a: req.a, b: req.b, width: 0.35e-3 }, "Line drawn.", "keep");
        return;
      case "label":
        await addItem({ kind: "label", at: req.at, text: req.text, size: IN / 8, leader: req.leader }, "Label placed.", "keep");
        return;
      case "dimension":
        await addItem({ kind: "dimension", viewport: req.viewport, dim: req.dim, points: req.points, offset: req.offset, style: "arrows" }, "Dimension placed: it measures the model and follows its viewport.", "keep");
        return;
    }
  };

  const placeImage = async (file: File) => {
    const rect = pendingImage.current;
    pendingImage.current = null;
    if (!rect || activeId === null) return;
    const bytes = new Uint8Array(await file.arrayBuffer());
    const mime = sniffImage(bytes);
    if (!mime) return message(`${file.name} is not a PNG or JPEG image.`, true);
    const path = texturePathFor(file.name, mime, props.texturePaths);
    const r = await apply(
      {
        op: "batch",
        ops: [
          { op: "add_texture", path, mime, data: toBase64(bytes) },
          { op: "add_sheet_item", sheet: activeId, item: { kind: "image", rect, path } },
        ],
      },
      () => `${file.name} placed (kept in the document).`,
    );
    const outs = r?.outcome.kind === "batch" ? r.outcome.outcomes : [];
    const last = outs[outs.length - 1];
    if (last?.kind === "sheet_item_added") {
      setToolState("select");
      setSelection([last.item]);
    }
  };

  const onFitScale = async (id: number) => {
    const item = shown?.sheet.items.find((i) => i.id === id);
    if (!item || item.kind !== "viewport") return;
    const scale = await fitScale(item);
    if (!scale) return message("Nothing to fit: the view is empty, or it is not a square view.", true);
    const auto = !item.caption || item.caption === captionFor(item.source, item.scale);
    setItem(id, { ...item, scale, caption: auto ? captionFor(item.source, scale) : item.caption }, `Viewport at ${scaleLabel(scale)}.`);
  };

  const onCurrentView = async (id: number) => {
    const item = shown?.sheet.items.find((i) => i.id === id);
    const cam = props.camera();
    if (!item || item.kind !== "viewport" || !cam) return;
    const source: ViewportSource = { from: "camera", camera: cam };
    let scale: Scale = { kind: "nts" };
    if (cam.parallel) scale = (await fitScale({ ...item, source, scale, centre: null })) ?? scale;
    setItem(id, { ...item, source, scale, centre: null, caption: item.caption === captionFor(item.source, item.scale) ? captionFor(source, scale) : item.caption }, `Viewport shows the 3D view${cam.parallel ? ` at ${scaleLabel(scale)}` : " (perspective: not to scale)"}.`);
  };

  // ---- exports (lane 12 step 6 adds PDF, DXF, PNG and printing) -------------------------
  const exportSheet = async (kind: SheetExportKind) => {
    const k = kernel();
    if (!k || activeId === null || !shown) return;
    const r = await k.sheetRender(activeId);
    const base = `${props.docName} - ${shown.sheet.name}`;
    try {
      if (kind === "svg") {
        // Images go in as data URLs, so the file stands alone.
        const urls = new Map<string, string>();
        for (const p of r.prims) {
          if (p.p !== "image" || urls.has(p.path)) continue;
          const bytes = await k.texture(p.path);
          const mime = sniffImage(bytes) ?? "image/png";
          urls.set(p.path, `data:${mime};base64,${toBase64(bytes)}`);
        }
        const svg = sheetSvg(r, (p) => urls.get(p) ?? null, shown.sheet.units ?? "inches");
        const res = await saveFile(`${base}.svg`, new TextEncoder().encode(svg), SVG_FILE);
        if (res) message(`Saved ${res.name}: the sheet at its real paper size.`);
      }
    } catch (e) {
      message(`Export refused: ${e instanceof Error ? e.message : String(e)}`, true);
    }
  };

  // ---- keys -----------------------------------------------------------------------------
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t && (t.isContentEditable || t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || t instanceof HTMLSelectElement)) return;
      const { render: r, selection: sel, activeId: id } = live.current;
      if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "a") {
        e.preventDefault();
        setSelection(r?.items.map((i) => i.id) ?? []);
        return;
      }
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      if (e.key === "Escape") {
        if (canvas.current?.cancel()) return;
        if (live.current.tool !== "select") return setTool("select");
        setSelection([]);
        return;
      }
      if ((e.key === "Delete" || e.key === "Backspace") && sel.length) {
        e.preventDefault();
        deleteItems(sel);
        return;
      }
      if (e.key.startsWith("Arrow") && sel.length && id !== null) {
        e.preventDefault();
        const g = shown?.sheet.grid || 0.001;
        const step = g * (e.shiftKey ? 10 : 1);
        const by: [number, number] = e.key === "ArrowLeft" ? [-step, 0] : e.key === "ArrowRight" ? [step, 0] : e.key === "ArrowUp" ? [0, -step] : [0, step];
        const items = sel.filter((x) => shown?.sheet.items.find((i) => i.id === x)?.kind !== "dimension");
        if (items.length) moveItems(items, by);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deleteItems, moveItems, shown]);

  // Read-only hooks for tests and captures.
  useEffect(() => {
    const w = window as unknown as { __sawhorse?: Record<string, unknown> };
    w.__sawhorse = {
      ...(w.__sawhorse ?? {}),
      sheet: {
        render: () => live.current.render,
        active: () => live.current.activeId,
        selection: () => live.current.selection,
        tool: () => live.current.tool,
        fit: () => canvas.current?.fit(),
        toClient: (p: [number, number]) => canvas.current?.toClient(p) ?? null,
      },
    };
  }, []);

  const snap = useCallback(
    async (item: number, at: [number, number], radius: number) => {
      const k = kernel();
      if (!k || activeId === null) return null;
      return k.sheetSnap(activeId, item, at[0], at[1], radius);
    },
    [kernel, activeId],
  );

  const exports = useMemo<SheetExportKind[]>(() => ["svg"], []);

  return (
    <>
      <SheetTabs
        lead={props.lead}
        sheets={sheets}
        active={activeId}
        onGo={(id) => {
          setActive(id);
          setSelection([]);
        }}
        onAdd={() => void addSheet()}
        onRename={(id, name) => {
          const row = sheets.find((s) => s.id === id);
          if (row) void apply({ op: "set_sheet", sheet: id, setup: { ...setupOf(row), name } }, () => `${row.sheet.name} renamed to ${name}.`);
        }}
        onMove={(id, to) => void apply({ op: "move_sheet", sheet: id, to }, () => `${sheetName(id)} moved.`)}
        onDelete={(id) => void apply({ op: "delete_sheet", sheet: id }, () => `${sheetName(id)} deleted (Ctrl+Z brings it back).`)}
      />
      <SheetRail active={tool} onSelect={setTool} />
      <section className="vp sheet-area" aria-label="Sheet">
        {shown ? (
          <SheetCanvas
            ref={canvas}
            render={render}
            sheet={shown.sheet}
            href={href}
            tool={tool}
            dimKind={dimKind}
            selection={selection}
            gridSnap={gridSnap}
            onSelection={setSelection}
            snap={snap}
            onMove={moveItems}
            onSet={(id, kind) => setItem(id, kind)}
            onPlace={(req) => void place(req)}
            onMessage={(t, err) => message(t, err)}
          />
        ) : (
          <div className="sheet-empty">
            <p>No drawing sheets yet.</p>
            <button type="button" className="btn primary" onClick={() => void addSheet()}>
              Add a sheet
            </button>
          </div>
        )}
        {shown ? (
          <div className="sheet-hud">
            <span>
              {shown.sheet.name} · {render ? `${render.items.length} item${render.items.length === 1 ? "" : "s"}` : "laying out…"}
            </span>
            <button type="button" aria-pressed={gridSnap} title="Snap to the sheet's grid" onClick={() => setGridSnap((g) => (keep(SNAP_KEY, g ? "off" : "on"), !g))}>
              Grid snap
            </button>
            <button type="button" title="Fit the page in the window" onClick={() => canvas.current?.fit()}>
              Fit
            </button>
          </div>
        ) : null}
        <input
          ref={imageInput}
          type="file"
          hidden
          accept="image/png,image/jpeg"
          data-testid="sheet-image-input"
          onChange={(e) => {
            const f = e.currentTarget.files?.[0];
            e.currentTarget.value = "";
            if (f) void placeImage(f);
            else pendingImage.current = null;
          }}
        />
      </section>
      <SheetsPane
        sheets={sheets}
        active={activeId}
        render={render}
        selection={selection}
        scenes={scenes}
        tool={tool}
        newSource={newSource}
        onNewSource={setNewSource}
        dimKind={dimKind}
        onDimKind={setDimKind}
        onSetup={setSetup}
        onSetItem={(id, kind) => setItem(id, kind)}
        onDeleteItems={deleteItems}
        onOrder={(id, to) => activeId !== null && void apply({ op: "order_sheet_item", sheet: activeId, item: id, to }, () => (to === 0 ? "Sent to the back." : "Brought to the front."))}
        onFitScale={(id) => void onFitScale(id)}
        onCurrentView={(id) => void onCurrentView(id)}
        onExport={(k) => void exportSheet(k)}
        exports={exports}
      />
    </>
  );
}
