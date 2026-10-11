import { useEffect, useRef, useState } from "react";
import { BrandMark } from "./glyphs";
import { VIEWS, type ViewId } from "../viewport/camera";

const MENUS = ["File", "Edit", "View", "Draw", "Tools", "Window", "Help"];

/** One entry in a drop-down menu ("-" is a separator). */
export type MenuItem =
  | "-"
  | {
      label: string;
      /** Shortcut shown on the right (display only; the app owns the keys). */
      keys?: string;
      checked?: boolean;
      disabled?: boolean;
      onSelect: () => void;
    };

export interface TopBarProps {
  /** The standard view the camera is on, or null after orbiting away. */
  view: ViewId | null;
  onView: (v: ViewId) => void;
  /** Credits on this device; null while the render service is not connected. */
  credits: number | null;
  /** Connection state of the render service (data attribute and tooltip). */
  creditsState?: string;
  creditsTitle?: string;
  /** Click on the chip (opens the Render pane). */
  onCredits?: () => void;
  /** Drop-down contents by menu name; menus without an entry stay inert, as in the mock. */
  menus?: Partial<Record<string, MenuItem[]>>;
}

export function TopBar({ view, onView, credits, creditsState, creditsTitle, onCredits, menus }: TopBarProps) {
  const [open, setOpen] = useState<string | null>(null);
  const navRef = useRef<HTMLElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: Event) => {
      if (e instanceof KeyboardEvent && e.key !== "Escape") return;
      if (e instanceof PointerEvent && navRef.current?.contains(e.target as Node)) return;
      setOpen(null);
    };
    window.addEventListener("pointerdown", close);
    window.addEventListener("keydown", close);
    return () => {
      window.removeEventListener("pointerdown", close);
      window.removeEventListener("keydown", close);
    };
  }, [open]);

  return (
    <header className="top">
      <div className="brand" title="Sawhorse">
        <BrandMark />
        Sawhorse
      </div>
      <nav className="menu" aria-label="Menu" ref={navRef}>
        {MENUS.map((m) => {
          const items = menus?.[m];
          return (
            <span key={m} style={{ position: "relative" }}>
              <button
                type="button"
                aria-haspopup={items ? "menu" : undefined}
                aria-expanded={items ? open === m : undefined}
                onClick={() => items && setOpen(open === m ? null : m)}
              >
                {m}
              </button>
              {items && open === m ? (
                <div className="drop" role="menu" aria-label={m}>
                  {items.map((it, i) =>
                    it === "-" ? (
                      <hr key={i} />
                    ) : (
                      <button
                        key={i}
                        type="button"
                        role={it.checked === undefined ? "menuitem" : "menuitemcheckbox"}
                        aria-checked={it.checked}
                        disabled={it.disabled}
                        onClick={() => {
                          setOpen(null);
                          it.onSelect();
                        }}
                      >
                        <span>
                          {it.checked === undefined ? "" : it.checked ? "✓ " : "   "}
                          {it.label}
                        </span>
                        {it.keys ? <kbd>{it.keys}</kbd> : null}
                      </button>
                    ),
                  )}
                </div>
              ) : null}
            </span>
          );
        })}
      </nav>
      <div className="spacer"></div>
      <div className="views" role="group" aria-label="Views">
        {VIEWS.map((v) => (
          <button key={v.id} type="button" title={v.title} aria-pressed={view === v.id} onClick={() => onView(v.id)}>
            {v.label}
          </button>
        ))}
      </div>
      <button type="button" className="chip" title={creditsTitle ?? "Render credits on this device"} data-state={creditsState} onClick={onCredits}>
        Credits <b id="credits">{credits ?? "—"}</b>
      </button>
    </header>
  );
}
