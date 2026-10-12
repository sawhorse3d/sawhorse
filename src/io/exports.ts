// The Export submenu: what each format is for, its file type, and what it needs.
import type { ExportFormat } from "../kernel/types";
import type { FileType } from "./files";

export interface ExportChoice {
  id: ExportFormat;
  label: string;
  /** One line for the status bar: what the file is for. */
  hint: string;
  type: FileType;
  /** "outline": a selected face or sketch region; "view": the current camera. */
  needs?: "outline" | "view";
}

const t = (description: string, mime: string, ext: string): FileType => ({ description, mime, extensions: [ext] });

export const EXPORTS: ExportChoice[] = [
  { id: "stl", label: "STL…", hint: "binary STL in millimetres for 3D printing (watertight solids only)", type: t("STL", "model/stl", ".stl") },
  { id: "3mf", label: "3MF…", hint: "3MF in millimetres, one part per solid, ready for a slicer (watertight solids only)", type: t("3MF", "model/3mf", ".3mf") },
  { id: "glb", label: "glTF (GLB)…", hint: "glTF binary in metres, Y up, one node per object, for viewers and game engines", type: t("glTF binary", "model/gltf-binary", ".glb") },
  { id: "obj", label: "OBJ…", hint: "Wavefront OBJ in millimetres with a group per object (and a .mtl when painted)", type: t("Wavefront OBJ", "model/obj", ".obj") },
  { id: "dxf", label: "DXF (3D)…", hint: "DXF in millimetres: 3DFACE faces and LINE edges, a layer per object", type: t("DXF", "image/vnd.dxf", ".dxf") },
  { id: "dxf-outline", label: "DXF outline of selected face…", hint: "the selected face's outline as closed polylines, 1:1 in millimetres, for laser and CNC", type: t("DXF", "image/vnd.dxf", ".dxf"), needs: "outline" },
  { id: "svg-view", label: "SVG drawing of this view…", hint: "a hidden-line drawing of the view; parallel views are 1:1 in millimetres", type: t("SVG", "image/svg+xml", ".svg"), needs: "view" },
  { id: "svg-outline", label: "SVG outline of selected face…", hint: "the selected face's outline, 1:1 in millimetres, for laser and CNC", type: t("SVG", "image/svg+xml", ".svg"), needs: "outline" },
];

export const MTL_TYPE: FileType = t("Material library", "model/mtl", ".mtl");
