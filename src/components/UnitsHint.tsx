/** localStorage key: once dismissed, the units hint stays away on this browser. */
export const UNITS_HINT_KEY = "sawhorse.unitsHint.dismissed";

/** The one sentence the browser demo shows a first-time visitor, above the unit chip. */
export const UNITS_HINT_TEXT = "Units live here: click the unit chip (or Edit > Units) to switch between millimetres, centimetres, metres, inches and feet-inches.";

export function unitsHintDismissed(): boolean {
  try {
    return localStorage.getItem(UNITS_HINT_KEY) === "1";
  } catch {
    return false;
  }
}

export function dismissUnitsHint(): void {
  try {
    localStorage.setItem(UNITS_HINT_KEY, "1");
  } catch {
    /* per-viewer convenience only: it comes back next visit */
  }
}

/**
 * The browser demo's first-run hint about units (lane 24, issue #13: a YouTuber's first look
 * at the web app "only gives me inches"). One sentence pinned above the unit chip; "Got it"
 * dismisses it for good on this browser, and so does opening the picker. The desktop app
 * never shows it (App renders it only when the build is the web demo).
 */
export function UnitsHint({ onDismiss }: { onDismiss: () => void }) {
  return (
    <aside className="units-hint" role="note" aria-label="Where the units are" data-testid="units-hint">
      <span>{UNITS_HINT_TEXT}</span>
      <button type="button" onClick={onDismiss}>
        Got it
      </button>
    </aside>
  );
}
