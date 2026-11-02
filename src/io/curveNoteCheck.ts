// Lane 27 (issue #17): does an import's curve note say what the model holds? Test tooling
// (src/io/skp.test.ts, src/io/skp-sweep.test.ts): the kernel's note "N curves came in as
// curves (a circles, b arcs, ...)" read back by kind, against the curves the view lists
// (`curves_json`) after one import into an empty document.

/** The counts by kind the note gives; `{ "?": N }` when it names one kind only (it then gives
 * the total alone), `null` when there is no such note. */
export function curveNoteCounts(notes: string[]): Record<string, number> | null {
  const line = notes.find((n) => /^\d+ curves? came in as curves?/.test(n));
  if (!line) return null;
  const total = Number(/^(\d+)/.exec(line)![1]);
  const out: Record<string, number> = {};
  const inner = /\(([^)]*)\)/.exec(line)?.[1] ?? "";
  for (const part of inner.split(/[,;]\s*/)) {
    const m = /^(\d+) (circle|arc|polygon|polyline)s?$/.exec(part.trim());
    if (m) out[m[2]] = Number(m[1]);
  }
  if (!Object.keys(out).length) out["?"] = total;
  return out;
}

/** True when the note counts exactly the curves in `view`, kind by kind. */
export function noteMatchesView(notes: string[], view: { kind: string }[]): boolean {
  const said = curveNoteCounts(notes);
  const seen: Record<string, number> = {};
  for (const c of view) seen[c.kind] = (seen[c.kind] ?? 0) + 1;
  if (!said) return view.length === 0;
  if ("?" in said) return Object.keys(seen).length === 1 && Object.values(seen)[0] === said["?"];
  const key = (r: Record<string, number>) => JSON.stringify(Object.entries(r).sort());
  return key(said) === key(seen);
}
