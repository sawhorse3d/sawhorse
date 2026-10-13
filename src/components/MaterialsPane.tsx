// The Materials pane (lane 10, ADR-0008): the active material, the materials in the model
// (with how many faces show each), the built-in library (woods, sheet goods, colours, glass
// and metal) as a searchable grid of swatches with their textures, "Import image..." to make
// a material from a picture, and an edit panel for the active model material: name, colour,
// texture size and rotation, and what it is as stock (the cut list reads the species).
import { useEffect, useMemo, useRef, useState } from "react";
import type { DocState, LibraryEntry, MaterialDef, MaterialInfo, StockLink } from "../kernel/types";
import { hexRgb, rgbHex, searchLibrary } from "../lib/looks";
import { formatLength, parseLength, type UnitSystem } from "../lib/units";

export type ActiveRef = { from: "model" | "library"; name: string } | null;

export interface MaterialsPaneProps {
  state: DocState | null;
  library: LibraryEntry[];
  /** An image URL for a swatch: a library material's texture, or a model texture path. */
  swatch: (s: { library?: string; texture?: string | null }) => string | null;
  active: ActiveRef;
  onActive: (a: ActiveRef) => void;
  /** Replace a model material's definition (live edits commit on change). */
  onDefine: (def: MaterialDef) => void;
  onRename: (name: string, to: string) => void;
  onDelete: (name: string) => void;
  /** Make a material from an image file. */
  onImport: (file: File) => void;
  /** Position the texture on the selected face (null: no single textured face selected). */
  onPosition: (() => void) | null;
  units: UnitSystem;
}

const CATEGORIES = ["Wood", "Sheet goods", "Colours", "Glass and metal"] as const;

function Swatch({ color, image, size = 34 }: { color: readonly number[]; image: string | null; size?: number }) {
  return (
    <span
      className="swatch"
      aria-hidden="true"
      style={{
        width: size,
        height: size,
        background: image ? `url(${image}) center / cover, ${rgbHex(color)}` : `rgba(${color[0]}, ${color[1]}, ${color[2]}, ${(color[3] ?? 255) / 255})`,
      }}
    ></span>
  );
}

/** Definition of a model material as an op carries it, with changes. */
export function defOf(m: MaterialInfo, change: Partial<MaterialDef> = {}): MaterialDef {
  return {
    name: m.name,
    color: m.color,
    texture: m.texture,
    size: m.size_set ? (m.size ?? null) : null,
    rotation: m.rotation ?? 0,
    offset: m.offset ?? [0, 0],
    stock: m.stock ?? null,
    ...change,
  };
}

export function MaterialsPane({ state, library, swatch, active, onActive, onDefine, onRename, onDelete, onImport, onPosition, units }: MaterialsPaneProps) {
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const models = state?.materials ?? [];
  const found = useMemo(() => searchLibrary(library, query, category), [library, query, category]);
  const current = active?.from === "model" ? models.find((m) => m.name === active.name) : undefined;
  const fromLib = active?.from === "library" ? library.find((e) => e.name === active.name) : undefined;
  const activeColor = current?.color ?? fromLib?.color ?? [200, 200, 200, 255];
  const activeImage = current ? (current.held ? swatch({ texture: current.texture }) : null) : fromLib ? swatch({ library: fromLib.name }) : null;

  return (
    <div className="materials" data-testid="materials">
      <div className="mat-active" data-active={active?.name ?? ""}>
        <Swatch color={activeColor} image={activeImage} size={40} />
        <div>
          <b>{active?.name ?? "No material"}</b>
          <small>{active ? (active.from === "library" ? "Library: added to the model when you paint with it" : `In the model${current?.used ? `, on ${current.used} face${current.used === 1 ? "" : "s"}` : ""}`) : "Pick one below, or Alt+click a face with Paint (B)."}</small>
        </div>
      </div>

      {models.length ? (
        <>
          <h4>In the model</h4>
          <div className="mat-grid" role="listbox" aria-label="Materials in the model">
            {models.map((m) => (
              <button
                key={m.name}
                type="button"
                role="option"
                className="mat"
                aria-selected={active?.from === "model" && active.name === m.name}
                title={`${m.name}${m.stock?.species ? ` (${m.stock.kind}: ${m.stock.species})` : ""}`}
                onClick={() => onActive({ from: "model", name: m.name })}
              >
                <Swatch color={m.color} image={m.held ? swatch({ texture: m.texture }) : null} />
                <span>{m.name}</span>
              </button>
            ))}
          </div>
        </>
      ) : null}

      {current ? <Editor key={current.name} m={current} onDefine={onDefine} onRename={onRename} onDelete={onDelete} onPosition={onPosition} units={units} taken={models.map((x) => x.name)} /> : null}

      <h4>Library</h4>
      <div className="mat-search">
        <input type="search" placeholder="Search: oak, ply, blue..." aria-label="Search the library" value={query} onChange={(e) => setQuery(e.target.value)} />
        <button type="button" className="mini" onClick={() => fileRef.current?.click()} title="Make a material from a PNG or JPEG">
          Import image...
        </button>
        <input
          ref={fileRef}
          type="file"
          hidden
          accept="image/png,image/jpeg,.png,.jpg,.jpeg"
          data-testid="material-image"
          onChange={(e) => {
            const f = e.currentTarget.files?.[0];
            e.currentTarget.value = "";
            if (f) onImport(f);
          }}
        />
      </div>
      <div className="chips" role="group" aria-label="Library categories">
        <button type="button" aria-pressed={category === null} onClick={() => setCategory(null)}>
          All
        </button>
        {CATEGORIES.map((c) => (
          <button key={c} type="button" aria-pressed={category === c} onClick={() => setCategory(category === c ? null : c)}>
            {c}
          </button>
        ))}
      </div>
      <div className="mat-grid" role="listbox" aria-label="Material library">
        {found.map((e) => (
          <button
            key={e.name}
            type="button"
            role="option"
            className="mat"
            aria-selected={active?.from === "library" && active.name === e.name}
            title={`${e.name}${e.stock ? ` (${e.stock.kind})` : ""}`}
            onClick={() => onActive({ from: "library", name: e.name })}
          >
            <Swatch color={e.color} image={e.texture ? swatch({ library: e.name }) : null} />
            <span>{e.name}</span>
          </button>
        ))}
        {!found.length ? <p className="note">Nothing in the library matches.</p> : null}
      </div>
      <p className="note">Paint (B): click a face to paint it, a closed group or component to paint what inside it has no paint of its own. Alt+click picks a material up. Double-click a textured face to move, turn or scale its texture.</p>
    </div>
  );
}

function Editor({ m, onDefine, onRename, onDelete, onPosition, units, taken }: { m: MaterialInfo; onDefine: (d: MaterialDef) => void; onRename: (a: string, b: string) => void; onDelete: (n: string) => void; onPosition: (() => void) | null; units: UnitSystem; taken: string[] }) {
  const [name, setName] = useState(m.name);
  const size = m.size ?? [0.3048, 0.3048];
  const [w, setW] = useState(formatLength(size[0], units));
  const [h, setH] = useState(formatLength(size[1], units));
  const [rot, setRot] = useState(String(Math.round((m.rotation ?? 0) * 10) / 10));
  const [species, setSpecies] = useState(m.stock?.species ?? "");
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => {
    setW(formatLength(size[0], units));
    setH(formatLength(size[1], units));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [m.size?.[0], m.size?.[1], units]);
  // Lane 15: every field follows the material when it changes elsewhere (undo, a rename,
  // another pane), so what the field shows is what the model holds.
  useEffect(() => setName(m.name), [m.name]);
  useEffect(() => setRot(String(Math.round((m.rotation ?? 0) * 10) / 10)), [m.rotation]);
  useEffect(() => setSpecies(m.stock?.species ?? ""), [m.stock?.species]);
  const commitSize = (which: 0 | 1, text: string) => {
    const r = parseLength(text, units);
    if (!r.ok || r.metres <= 0) {
      setProblem(r.ok ? "A texture needs a size above zero." : r.error);
      return;
    }
    setProblem(null);
    // Unchanged (Enter, then the blur that follows it): nothing to commit, no undo step.
    if (Math.abs(r.metres - size[which]) <= 1e-9) {
      if (which === 0) setW(formatLength(size[0], units));
      else setH(formatLength(size[1], units));
      return;
    }
    // The other side follows to keep the image's shape.
    const k = r.metres / size[which];
    const next: [number, number] = which === 0 ? [r.metres, size[1] * k] : [size[0] * k, r.metres];
    onDefine(defOf(m, { size: next }));
  };
  const stockKind = m.stock?.kind ?? "none";
  const setStock = (kind: string, sp: string) => {
    const stock: StockLink | null = kind === "none" ? null : { kind: kind as StockLink["kind"], species: sp.trim() };
    onDefine(defOf(m, { stock }));
  };
  return (
    <div className="mat-edit" data-testid="material-editor">
      <label>
        Name
        <input
          value={name}
          aria-label="Material name"
          onChange={(e) => setName(e.target.value)}
          onBlur={() => {
            const t = name.trim();
            if (t && t !== m.name && !taken.includes(t)) onRename(m.name, t);
            else setName(m.name);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") (e.target as HTMLInputElement).blur();
          }}
        />
      </label>
      <label>
        Colour
        <input type="color" aria-label="Material colour" value={rgbHex(m.color)} onChange={(e) => onDefine(defOf(m, { color: [...hexRgb(e.target.value), m.color[3]] as [number, number, number, number] }))} />
      </label>
      {m.held ? (
        <>
          <label>
            Texture width
            <input aria-label="Texture width" value={w} onChange={(e) => setW(e.target.value)} onBlur={() => commitSize(0, w)} onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()} />
          </label>
          <label>
            Height
            <input aria-label="Texture height" value={h} onChange={(e) => setH(e.target.value)} onBlur={() => commitSize(1, h)} onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()} />
          </label>
          <label>
            Rotation
            <input
              aria-label="Texture rotation"
              type="number"
              step={15}
              value={rot}
              onChange={(e) => setRot(e.target.value)}
              onBlur={() => {
                const v = Number(rot);
                if (rot.trim() === "" || !Number.isFinite(v)) setRot(String(Math.round((m.rotation ?? 0) * 10) / 10));
                else if (Math.abs(v - (m.rotation ?? 0)) > 1e-9) onDefine(defOf(m, { rotation: v }));
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") (e.target as HTMLInputElement).blur();
              }}
            />
          </label>
        </>
      ) : m.texture ? (
        <p className="note">Texture {m.texture} was not in the file: the colour shows instead.</p>
      ) : null}
      <label>
        Stock
        <select aria-label="Stock kind" value={stockKind} onChange={(e) => setStock(e.target.value, species || m.name)}>
          <option value="none">Not stock</option>
          <option value="lumber">Solid lumber</option>
          <option value="sheet">Sheet goods</option>
        </select>
      </label>
      {stockKind !== "none" ? (
        <label>
          Species
          <input
            aria-label="Species"
            value={species}
            placeholder={m.name}
            onChange={(e) => setSpecies(e.target.value)}
            onBlur={() => {
              if (species.trim() !== (m.stock?.species ?? "")) setStock(stockKind, species);
            }}
            onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
          />
        </label>
      ) : null}
      {problem ? <p className="note err">{problem}</p> : null}
      <div className="row">
        <button
          type="button"
          className="mini"
          disabled={!onPosition || !m.held}
          onClick={() => onPosition?.()}
          title={!m.held ? "This material has no texture image in the model to position" : !onPosition ? "Select one face showing this texture first" : "Move, turn and scale the texture on the selected face with handles in the view (or double-click a textured face)"}
        >
          Position texture
        </button>
        <button type="button" className="mini danger" onClick={() => onDelete(m.name)}>
          Delete
        </button>
      </div>
    </div>
  );
}
