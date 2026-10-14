// Lane 20b: where the Cut list pane's text may wrap. Nothing in the pane's table ends in an
// ellipsis: a part's name wraps between words, and inside a word only at a joint a person
// would break it at ("Agrupar" | "#8", "Laura_" | "Skin", "drawer" | "Front"); a size wraps
// between its whole number and its fraction, the fraction as one unit ("22" | "13/16"); a
// note wraps between notes and words. A single token of 30 or more characters may, as the
// last resort, break anywhere (the browser does that only where nothing else fits).

/** A name token at least this long may break anywhere when it fits nowhere else. */
export const LONG_TOKEN = 30;

const isLower = (c: string | undefined) => !!c && /\p{Ll}/u.test(c);
const isUpper = (c: string | undefined) => !!c && /\p{Lu}/u.test(c);
const isSpace = (c: string | undefined) => !!c && /\s/u.test(c);

/**
 * The pieces of `text` with a wrap allowed between each two (the pane puts a `<wbr>` there):
 * before "#" ("Agrupar" | "#8"), after a run of "_" or "-" ("Laura_" | "Skin"), and at a
 * camelCase joint ("drawer" | "Front", "XML" | "Parser"); never at the start of a token, and
 * never inside "(3D)shelf2B" (no joint). Spaces stay inside their piece, since a line breaks
 * at a space anyway. Joined, the pieces are `text`.
 */
export function nameBreaks(text: string): string[] {
  const chars = Array.from(text);
  const out: string[] = [];
  let cur = "";
  for (let i = 0; i < chars.length; i++) {
    const c = chars[i];
    const prev = chars[i - 1];
    const next = chars[i + 1];
    let joint = false;
    if (cur && prev !== undefined && !isSpace(prev) && !isSpace(c)) {
      if (c === "#") joint = prev !== "#";
      else if (prev === "_" || prev === "-") joint = c !== "_" && c !== "-";
      else if (isLower(prev) && isUpper(c)) joint = true;
      else if (isUpper(prev) && isUpper(c) && isLower(next)) joint = true;
    }
    if (joint) {
      out.push(cur);
      cur = "";
    }
    cur += c;
  }
  if (cur) out.push(cur);
  return out;
}

/** One run of a name: a token of `LONG_TOKEN` or more characters (`long`) or the text
 * between them; each as its pieces (`nameBreaks`). */
export interface NameRun {
  long: boolean;
  pieces: string[];
}

/** A name as runs: whitespace and ordinary tokens together, each long token on its own. */
export function nameRuns(text: string): NameRun[] {
  const runs: NameRun[] = [];
  let plain = "";
  for (const part of text.split(/(\s+)/u)) {
    if (!part) continue;
    if (!/\s/u.test(part) && Array.from(part).length >= LONG_TOKEN) {
      if (plain) runs.push({ long: false, pieces: nameBreaks(plain) });
      plain = "";
      runs.push({ long: true, pieces: nameBreaks(part) });
    } else plain += part;
  }
  if (plain) runs.push({ long: false, pieces: nameBreaks(plain) });
  return runs;
}

/** A size's units of wrapping: "22 13/16" is "22" and "13/16"; "2438.5" and "13/16" are one
 * each. Each unit stays on one line; a line breaks only between them. */
export function sizePieces(text: string): string[] {
  return text.split(" ").filter((x) => x.length > 0);
}

/** The width (characters) a size column needs for `text`: the whole size when it is no
 * longer than `cap`, else its widest unit (it then wraps between its units). */
export function sizeChars(text: string, cap: number): number {
  const widest = Math.max(1, ...sizePieces(text).map((x) => x.length));
  return Math.max(widest, Math.min(cap, text.length));
}
