// The 3D view's right-click menu (#107). It holds only commands the menu bar already has,
// taken from the menu bar's own items, so a command has the same name, keys, disabled rule
// and action in both places. Which commands show depends on what was right-clicked: the
// selection, a section plane, or empty space; inside an open group or component, Close
// Group / Component is added. Escape, a press outside it, the wheel, the window losing focus
// or resized, or a choice closes it (and the app closes it when the model, the selection or
// the tool changes under it).
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { isMacControlClick } from "../lib/platform";
import type { MenuItem } from "./TopBar";

/** What a right-click in the view was on. */
export type ViewMenuTarget = "selection" | "section" | "empty";

/** The menu bar commands each kind of right-click offers, in order ("-": a separator). A
 * command the menu bar does not show at the moment (the section plane's three, only while
 * one plane is selected) is left out. */
const OFFER: Record<ViewMenuTarget, string[]> = {
  selection: ["Edit Group / Component", "-", "Delete", "Hide", "Lock", "-", "Make Group", "Make Component…", "Make Unique", "Explode", "Explode Curve", "Clear Paint"],
  section: ["Activate Cut", "Deactivate Cut", "Reverse Section Plane", "Create Group from Slice", "-", "Delete"],
  empty: ["Select All", "Select None", "-", "Zoom Extents", "-", "Undo", "Redo"],
};

type Command = Exclude<MenuItem, "-" | { items: MenuItem[] }>;

/** The menu bar's command named `name` ("Undo" also matches "Undo Push/Pull"). */
function command(menus: Record<string, MenuItem[]>, name: string): Command | null {
  for (const list of Object.values(menus)) {
    for (const it of list) {
      if (it === "-" || "items" in it) continue;
      if (it.label === name || (name === "Undo" && it.label.startsWith("Undo"))) return it;
    }
  }
  return null;
}

/** The right-click menu for `target`, from the menu bar's own items. Separators only between
 * commands. `inContext`: a group or component is open, so Close Group / Component is offered. */
export function viewMenuItems(target: ViewMenuTarget, menus: Record<string, MenuItem[]>, inContext: boolean): MenuItem[] {
  const names = [...OFFER[target], ...(inContext ? ["-", "Close Group / Component"] : [])];
  const out: MenuItem[] = [];
  for (const name of names) {
    if (name === "-") {
      if (out.length && out[out.length - 1] !== "-") out.push("-");
      continue;
    }
    const c = command(menus, name);
    if (c) out.push(c);
  }
  while (out[out.length - 1] === "-") out.pop();
  return out;
}

export interface ViewMenuProps {
  /** Page pixels where the right-click was. */
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
  /** The 3D view: a press or the wheel there that closes the menu goes no further. */
  view?: () => Element | null;
}

export function ViewMenu({ x, y, items, onClose, view }: ViewMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState({ left: x, top: y });
  // Kept inside the window: a right-click near the right or bottom edge opens it to the left
  // or above.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setAt({ left: Math.max(0, Math.min(x, window.innerWidth - r.width)), top: Math.max(0, Math.min(y, window.innerHeight - r.height)) });
  }, [x, y]);
  useEffect(() => {
    const away = (e: Event) => {
      if (ref.current && e.target instanceof Node && ref.current.contains(e.target)) return;
      // A press or the wheel on the 3D view only closes the menu, as Escape does: it does not
      // also reach the tool or the camera (a click would select or place a point, a middle
      // drag orbit, the wheel zoom). A right press (on macOS a Control+click) goes on, so it
      // opens the menu again where it is.
      const v = view?.();
      const reopens = e instanceof MouseEvent && e.type === "pointerdown" && (e.button === 2 || isMacControlClick(e));
      if (v && e.target instanceof Node && v.contains(e.target) && !reopens) {
        e.stopPropagation();
        e.preventDefault();
      }
      onClose();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      // Only the menu closes: Escape does not also clear the selection or close a group.
      e.stopPropagation();
      e.preventDefault();
      onClose();
    };
    window.addEventListener("pointerdown", away, true);
    // Not passive, so the wheel on the view can be held back (see away).
    window.addEventListener("wheel", away, { capture: true, passive: false });
    window.addEventListener("keydown", key, true);
    window.addEventListener("blur", onClose);
    // The window resized: the place it was kept inside the window for is gone.
    window.addEventListener("resize", onClose);
    return () => {
      window.removeEventListener("resize", onClose);
      window.removeEventListener("pointerdown", away, true);
      window.removeEventListener("wheel", away, true);
      window.removeEventListener("keydown", key, true);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose, view]);
  return (
    <div className="ctxmenu" ref={ref} role="menu" aria-label="View actions" data-testid="view-menu" style={at} onContextMenu={(e) => e.preventDefault()}>
      {items.map((it, i) =>
        it === "-" ? (
          <hr key={`-${i}`} />
        ) : "items" in it ? null : (
          <button
            key={it.label}
            type="button"
            role="menuitem"
            disabled={it.disabled}
            title={it.title}
            onClick={() => {
              onClose();
              it.onSelect();
            }}
          >
            {it.label} {it.keys ? <kbd>{it.keys}</kbd> : null}
          </button>
        ),
      )}
    </div>
  );
}
