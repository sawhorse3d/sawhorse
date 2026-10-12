// The cut list's Round to choice between visits (lane 7b). "Follow units" is the default.
// A pick lasts for the session (sessionStorage, so a reload keeps it); pinned, it is also
// kept for next time (localStorage). Both are per-viewer conveniences: blocked storage
// just means Follow units.
import { isRoundingChoice, type RoundingChoice } from "./format";

export const CUT_ROUNDING_KEY = "sawhorse.cutRounding";

function read(store: () => Storage): RoundingChoice | null {
  try {
    const v = store().getItem(CUT_ROUNDING_KEY);
    return isRoundingChoice(v) ? v : null;
  } catch {
    return null;
  }
}

function write(store: () => Storage, v: RoundingChoice | null): void {
  try {
    if (v === null || v === "follow") store().removeItem(CUT_ROUNDING_KEY);
    else store().setItem(CUT_ROUNDING_KEY, v);
  } catch {
    /* storage blocked: the choice lasts until the page closes */
  }
}

/** The choice at start: this session's pick, else the pinned one, else Follow units. */
export function loadCutRounding(): { choice: RoundingChoice; pinned: boolean } {
  const pinned = read(() => localStorage);
  const session = read(() => sessionStorage);
  return { choice: session ?? pinned ?? "follow", pinned: pinned !== null };
}

/** Remember a choice for the session, and for next time when `pinned` (unpinned: forget
 * any pinned one). */
export function saveCutRounding(choice: RoundingChoice, pinned: boolean): void {
  write(() => sessionStorage, choice);
  write(() => localStorage, pinned ? choice : null);
}
