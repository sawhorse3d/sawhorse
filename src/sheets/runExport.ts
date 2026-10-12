// One place that turns "export this sheet as ..." into a saved file (lane 12): the Sheet
// pane's buttons and the File menu both come here.
import { PDF_TYPE } from "../cutlist/pdf";
import { saveFile, type FileType } from "../io/files";
import type { KernelClient } from "../kernel/client";
import type { SheetRow } from "../kernel/sheetTypes";
import { printSheets, sheetImages, sheetPng, sheetsPdf, sheetSvgFile } from "./exports";

export type SheetExportKind = "pdf" | "pdf-all" | "svg" | "dxf" | "png150" | "png300" | "print" | "print-all";

export const SVG_TYPE: FileType = { description: "SVG drawing", mime: "image/svg+xml", extensions: [".svg"] };
export const DXF_TYPE: FileType = { description: "DXF drawing", mime: "image/vnd.dxf", extensions: [".dxf"] };
export const PNG_TYPE: FileType = { description: "PNG image", mime: "image/png", extensions: [".png"] };

/** File names never carry characters a file system refuses. */
export function safeName(s: string): string {
  return s.replace(/[\\/:*?"<>|]+/g, "-").replace(/\s+/g, " ").trim() || "Sheet";
}

export interface SheetExportContext {
  kernel: KernelClient;
  sheets: SheetRow[];
  /** The sheet the one-sheet exports write. */
  active: number | null;
  docName: string;
  message: (text: string, error?: boolean) => void;
}

/** Write (or print) sheets; reports what happened through `message`. */
export async function runSheetExport(kind: SheetExportKind, ctx: SheetExportContext): Promise<void> {
  const { kernel: k, sheets, docName, message } = ctx;
  const all = kind === "pdf-all" || kind === "print-all";
  const rows = all ? sheets : sheets.filter((s) => s.id === (ctx.active ?? sheets[0]?.id));
  if (!rows.length) return message("There is no sheet to export: add one in Sheets mode.", true);
  const one = rows[0];
  const units = one.sheet.units ?? "inches";
  const base = safeName(all ? `${docName} sheets` : `${docName} - ${one.sheet.name}`);
  try {
    if (kind === "dxf") {
      const text = await k.sheetDxf(one.id);
      const res = await saveFile(`${base}.dxf`, new TextEncoder().encode(text), DXF_TYPE);
      if (res) message(`Saved ${res.name}: lines, outlines and text at the paper's size in ${units === "inches" ? "inches" : "millimetres"}, a layer per kind of thing.`);
      return;
    }
    const renders = [];
    for (const r of rows) renders.push(await k.sheetRender(r.id));
    const images = await sheetImages(renders, (p) => k.texture(p));
    if (kind === "pdf" || kind === "pdf-all") {
      const bytes = sheetsPdf(renders, images, all ? docName : `${docName}: ${one.sheet.name}`);
      const res = await saveFile(`${base}.pdf`, bytes, PDF_TYPE);
      if (res) message(`Saved ${res.name}: ${renders.length === 1 ? "the sheet" : `${renders.length} sheets, a page each`}, vector, at the paper's size. Print it at 100% to keep the scales.`);
    } else if (kind === "svg") {
      const svg = sheetSvgFile(renders[0], images, units);
      const res = await saveFile(`${base}.svg`, new TextEncoder().encode(svg), SVG_TYPE);
      if (res) message(`Saved ${res.name}: the sheet at its real paper size.`);
    } else if (kind === "png150" || kind === "png300") {
      const dpi = kind === "png150" ? 150 : 300;
      const png = await sheetPng(renders[0], images, units, dpi);
      const res = await saveFile(`${base}.png`, png, PNG_TYPE);
      if (res) message(`Saved ${res.name}: ${dpi} dpi.`);
    } else {
      printSheets(renders, images, units);
      message(`Printing ${renders.length === 1 ? "the sheet" : `${renders.length} sheets`} at 100% (the system's print dialog can also save a PDF).`);
    }
  } catch (e) {
    message(`Export refused: ${e instanceof Error ? e.message : String(e)}`, true);
  }
}
