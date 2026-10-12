// A small PDF 1.4 writer (lane 7), our own: pages of text, lines and rectangles in the
// standard Helvetica fonts (no embedding: every PDF reader has them), uncompressed content
// streams, a cross-reference table with exact byte offsets. Enough for the cut list report
// and label sheets; nothing else depends on it.
//
// Coordinates are PDF points (1/72 in) from the bottom-left corner of the page.

/** Helvetica advance widths (1/1000 em) for ASCII 32..126, from the font's public metrics. */
const HELVETICA = [
  278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667,
  556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500,
  500, 500, 334, 260, 334, 584,
];

/** WinAnsi codes for the few non-ASCII characters the cut list writes. */
const WIN_ANSI: Record<string, number> = { "×": 0xd7, "·": 0xb7, "—": 0x97, "–": 0x96, "²": 0xb2, "½": 0xbd, "¼": 0xbc, "¾": 0xbe, "°": 0xb0, "’": 0x92, "“": 0x93, "”": 0x94, "é": 0xe9, "è": 0xe8, "ü": 0xfc, "ö": 0xf6, "ä": 0xe4 };
const WIDE: Record<number, number> = { 0xd7: 584, 0xb7: 278, 0x97: 1000, 0x96: 556, 0xb2: 333, 0xbd: 834, 0xbc: 834, 0xbe: 834, 0xb0: 400, 0x92: 222, 0x93: 333, 0x94: 333 };

/** A string as WinAnsi codes (unknown characters become "?"). */
export function winAnsi(s: string): number[] {
  const out: number[] = [];
  for (const ch of s) {
    const c = ch.codePointAt(0)!;
    if (c >= 32 && c <= 126) out.push(c);
    else if (WIN_ANSI[ch] !== undefined) out.push(WIN_ANSI[ch]);
    else out.push(63);
  }
  return out;
}

/** Width of `s` in points at `size` (Helvetica; bold runs about 5% wider). */
export function textWidth(s: string, size: number, bold = false): number {
  let w = 0;
  for (const c of winAnsi(s)) w += c >= 32 && c <= 126 ? HELVETICA[c - 32] : (WIDE[c] ?? 556);
  return (w * size * (bold ? 1.05 : 1)) / 1000;
}

/** `s` shortened with "..." to fit `width` points. */
export function fit(s: string, size: number, width: number, bold = false): string {
  if (textWidth(s, size, bold) <= width) return s;
  let t = s;
  while (t.length > 1 && textWidth(`${t}...`, size, bold) > width) t = t.slice(0, -1);
  return `${t.trimEnd()}...`;
}

function pdfString(s: string): string {
  let out = "(";
  for (const c of winAnsi(s)) {
    if (c === 0x28 || c === 0x29 || c === 0x5c) out += `\\${String.fromCharCode(c)}`;
    else if (c < 32 || c > 126) out += `\\${c.toString(8).padStart(3, "0")}`;
    else out += String.fromCharCode(c);
  }
  return `${out})`;
}

const num = (n: number) => {
  const r = Math.round(n * 100) / 100;
  return Object.is(r, -0) ? "0" : String(r);
};

export class PdfPage {
  readonly ops: string[] = [];
  constructor(
    readonly width: number,
    readonly height: number,
  ) {}

  text(x: number, y: number, s: string, size = 10, opts: { bold?: boolean; align?: "left" | "right" | "center"; gray?: number } = {}): this {
    const w = textWidth(s, size, opts.bold);
    const dx = opts.align === "right" ? -w : opts.align === "center" ? -w / 2 : 0;
    const g = opts.gray ?? 0;
    this.ops.push(`BT ${num(g)} g /${opts.bold ? "F2" : "F1"} ${num(size)} Tf ${num(x + dx)} ${num(y)} Td ${pdfString(s)} Tj ET`);
    return this;
  }

  line(x1: number, y1: number, x2: number, y2: number, width = 0.5, gray = 0): this {
    this.ops.push(`${num(gray)} G ${num(width)} w ${num(x1)} ${num(y1)} m ${num(x2)} ${num(y2)} l S`);
    return this;
  }

  rect(x: number, y: number, w: number, h: number, opts: { stroke?: number; fill?: number; width?: number } = {}): this {
    const parts: string[] = [];
    if (opts.fill !== undefined) parts.push(`${num(opts.fill)} g`);
    if (opts.stroke !== undefined) parts.push(`${num(opts.stroke)} G ${num(opts.width ?? 0.5)} w`);
    parts.push(`${num(x)} ${num(y)} ${num(w)} ${num(h)} re`);
    parts.push(opts.fill !== undefined && opts.stroke !== undefined ? "B" : opts.fill !== undefined ? "f" : "S");
    this.ops.push(parts.join(" "));
    return this;
  }

  /** An arrow from (x1, y1) to (x2, y2) with a small head. */
  arrow(x1: number, y1: number, x2: number, y2: number, width = 0.8, head = 4): this {
    this.line(x1, y1, x2, y2, width);
    const len = Math.hypot(x2 - x1, y2 - y1) || 1;
    const [ux, uy] = [(x2 - x1) / len, (y2 - y1) / len];
    const [px, py] = [-uy, ux];
    this.line(x2, y2, x2 - ux * head + px * head * 0.6, y2 - uy * head + py * head * 0.6, width);
    this.line(x2, y2, x2 - ux * head - px * head * 0.6, y2 - uy * head - py * head * 0.6, width);
    return this;
  }

  content(): string {
    return this.ops.join("\n");
  }
}

/** US Letter, points. */
export const LETTER: [number, number] = [612, 792];

export class PdfDoc {
  readonly pages: PdfPage[] = [];
  constructor(readonly info: { title?: string; creator?: string } = {}) {}

  page(size: [number, number] = LETTER): PdfPage {
    const p = new PdfPage(size[0], size[1]);
    this.pages.push(p);
    return p;
  }

  /** The file. Deterministic: the same pages give the same bytes. */
  bytes(): Uint8Array {
    const objects: string[] = [];
    // Object numbers start at 1: push returns the new length, which is the new number.
    const add = (body: string) => objects.push(body);
    const catalog = add("");
    const pagesObj = add("");
    const font1 = add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>");
    const font2 = add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>");
    const kids: number[] = [];
    for (const p of this.pages) {
      const content = p.content();
      const stream = add(`<< /Length ${latin1Length(content)} >>\nstream\n${content}\nendstream`);
      kids.push(add(`<< /Type /Page /Parent ${pagesObj} 0 R /MediaBox [0 0 ${num(p.width)} ${num(p.height)}] /Resources << /Font << /F1 ${font1} 0 R /F2 ${font2} 0 R >> >> /Contents ${stream} 0 R >>`));
    }
    objects[catalog - 1] = `<< /Type /Catalog /Pages ${pagesObj} 0 R >>`;
    objects[pagesObj - 1] = `<< /Type /Pages /Kids [${kids.map((k) => `${k} 0 R`).join(" ")}] /Count ${kids.length} >>`;
    const infoObj = add(`<< /Title ${pdfString(this.info.title ?? "Cut list")} /Creator ${pdfString(this.info.creator ?? "Sawhorse")} /Producer ${pdfString("Sawhorse")} >>`);
    let out = "%PDF-1.4\n%âãÏÓ\n";
    const offsets: number[] = [];
    objects.forEach((body, i) => {
      offsets.push(latin1Length(out));
      out += `${i + 1} 0 obj\n${body}\nendobj\n`;
    });
    const xref = latin1Length(out);
    out += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
    for (const o of offsets) out += `${String(o).padStart(10, "0")} 00000 n \n`;
    out += `trailer\n<< /Size ${objects.length + 1} /Root ${catalog} 0 R /Info ${infoObj} 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
    return latin1(out);
  }
}

function latin1Length(s: string): number {
  return s.length; // every character written is a single byte (WinAnsi escapes are ASCII)
}

function latin1(s: string): Uint8Array {
  const b = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) b[i] = s.charCodeAt(i) & 0xff;
  return b;
}

export const PDF_TYPE = { description: "PDF document", mime: "application/pdf", extensions: [".pdf"] };

// ---- reading our own PDFs back (tests, and a sanity check before saving) ----------------

export interface PdfCheck {
  pages: number;
  /** Every text string drawn, page by page, in drawing order. */
  text: string[][];
}

/** Check a PDF this writer made: header, every xref offset pointing at its object, the
 * trailer, the page count; and read back its text. Throws on anything wrong. */
export function readPdf(bytes: Uint8Array): PdfCheck {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  if (!s.startsWith("%PDF-1.4\n")) throw new Error("not a PDF 1.4 header");
  if (!s.trimEnd().endsWith("%%EOF")) throw new Error("no %%EOF");
  const sx = /startxref\n(\d+)\n%%EOF\s*$/.exec(s);
  if (!sx) throw new Error("no startxref");
  const xref = Number(sx[1]);
  if (!s.startsWith("xref\n", xref)) throw new Error("startxref does not point at the xref table");
  const head = /^xref\n0 (\d+)\n/.exec(s.slice(xref));
  if (!head) throw new Error("bad xref header");
  const n = Number(head[1]);
  const table = s.slice(xref + head[0].length).split("\n").slice(0, n);
  for (let i = 1; i < n; i++) {
    const off = Number(table[i].slice(0, 10));
    if (!s.startsWith(`${i} 0 obj\n`, off)) throw new Error(`xref entry ${i} does not point at object ${i}`);
  }
  const objAt = (k: number) => {
    const off = Number(table[k].slice(0, 10));
    const end = s.indexOf("\nendobj\n", off);
    return s.slice(off, end);
  };
  const root = /\/Root (\d+) 0 R/.exec(s.slice(xref));
  if (!root) throw new Error("no root");
  const pagesRef = /\/Pages (\d+) 0 R/.exec(objAt(Number(root[1])));
  if (!pagesRef) throw new Error("no pages");
  const pagesObj = objAt(Number(pagesRef[1]));
  const kids = [...pagesObj.matchAll(/(\d+) 0 R/g)].map((m) => Number(m[1]));
  const count = Number(/\/Count (\d+)/.exec(pagesObj)?.[1] ?? -1);
  if (count !== kids.length) throw new Error("page count does not match the kids");
  const text = kids.map((k) => {
    const c = /\/Contents (\d+) 0 R/.exec(objAt(k));
    if (!c) throw new Error("page without contents");
    const stream = objAt(Number(c[1]));
    const len = Number(/\/Length (\d+)/.exec(stream)?.[1]);
    const start = stream.indexOf("stream\n") + 7;
    const body = stream.slice(start, start + len);
    if (stream.slice(start + len, start + len + 10) !== "\nendstream") throw new Error("stream length is wrong");
    return [...body.matchAll(/\((?:[^()\\]|\\.)*\) Tj/g)].map((m) => unescape(m[0].slice(1, -4)));
  });
  return { pages: kids.length, text };
}

const FROM_ANSI = Object.fromEntries(Object.entries(WIN_ANSI).map(([ch, c]) => [c, ch]));

function unescape(s: string): string {
  let out = "";
  for (let i = 0; i < s.length; i++) {
    if (s[i] !== "\\") {
      out += s[i];
      continue;
    }
    const oct = /^[0-7]{3}/.exec(s.slice(i + 1));
    if (oct) {
      const c = parseInt(oct[0], 8);
      out += FROM_ANSI[c] ?? String.fromCharCode(c);
      i += 3;
    } else {
      out += s[i + 1];
      i += 1;
    }
  }
  return out;
}
