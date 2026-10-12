import { useState } from "react";
import { RELEASES_URL } from "../lib/site";

/** localStorage key: once dismissed, the banner stays away on this browser. */
export const DEMO_BANNER_KEY = "sawhorse.demoBanner.dismissed";

function wasDismissed(): boolean {
  try {
    return localStorage.getItem(DEMO_BANNER_KEY) === "1";
  } catch {
    return false;
  }
}

/**
 * The browser demo's one-line notice: alpha, runs here, nothing leaves the machine, and
 * where the desktop app is. Floats over the bottom of the viewport so the layout (and every
 * test that clicks the canvas) is unchanged; Dismiss hides it for good on this browser.
 */
export function DemoBanner() {
  const [open, setOpen] = useState(() => !wasDismissed());
  if (!open) return null;
  return (
    <aside className="demo-banner" role="note" aria-label="About this demo">
      <b>Alpha</b>
      <span>Runs in your browser, nothing is uploaded.</span>
      <a href={RELEASES_URL} target="_blank" rel="noopener noreferrer">
        Download the desktop app
      </a>
      <button
        type="button"
        aria-label="Dismiss"
        title="Dismiss"
        onClick={() => {
          setOpen(false);
          try {
            localStorage.setItem(DEMO_BANNER_KEY, "1");
          } catch {
            /* per-viewer convenience only: it comes back next visit */
          }
        }}
      >
        ×
      </button>
    </aside>
  );
}
