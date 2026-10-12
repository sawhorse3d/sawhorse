import { useEffect, useRef, useState } from "react";
import { BrandMark } from "./glyphs";
import { VIEWS, type ViewId } from "../viewport/camera";

const MENUS = ["File", "Edit", "View", "Draw", "Tools", "Window", "Help"];

/** One entry in a drop-down menu ("-" is a separator; `items` opens a submenu). */
export type MenuItem =
  | "-"
  | {
      label: string;
      /** Shortcut shown on the right (display only; the app owns the keys). */
      keys?: string;
      checked?: boolean;
      disabled?: boolean;
      /** Shown as a tooltip and to screen readers. */
      title?: string;
      onSelect: () => void;
    }
  | { label: string; items: MenuItem[]; disabled?: boolean };

export interface TopBarProps {
  /** The standard view the camera is on, or null after orbiting away. */
  view: ViewId | null;
  onView: (v: ViewId) => void;
  credits: number;
  /** Drop-down contents by menu name; menus without an entry stay inert, as in the mock. */
  menus?: Partial<Record<string, MenuItem[]>>;
}

function Entry({ item: it, sub, setSub, close }: { item: MenuItem; sub: string | null; setSub: (s: string | null) => void; close: () => void }) {
  if (it === "-") return <hr />;
  if ("items" in it) {
    const isOpen = sub === it.label;
    return (
      <span className="subwrap" onMouseEnter={() => !it.disabled && setSub(it.label)}>
        <button type="button" role="menuitem" aria-haspopup="menu" aria-expanded={isOpen} disabled={it.disabled} onClick={() => setSub(isOpen ? null : it.label)}>
          <span>{it.label}</span>
          <kbd aria-hidden="true">▸</kbd>
        </button>
        {isOpen ? (
          <div className="drop sub" role="menu" aria-label={it.label}>
            {it.items.map((x, k) => (
              <Entry key={k} item={x} sub={null} setSub={() => {}} close={close} />
            ))}
          </div>
        ) : null}
      </span>
    );
  }
  return (
    <button
      type="button"
      role={it.checked === undefined ? "menuitem" : "menuitemcheckbox"}
      aria-checked={it.checked}
      disabled={it.disabled}
      title={it.title}
      onMouseEnter={() => setSub(null)}
      onClick={() => {
        close();
        it.onSelect();
      }}
    >
      <span>
        {it.checked === undefined ? "" : it.checked ? "✓ " : "   "}
        {it.label}
      </span>
      {it.keys ? <kbd>{it.keys}</kbd> : null}
    </button>
  );
}

export function TopBar({ view, onView, credits, menus }: TopBarProps) {
  const [open, setOpen] = useState<string | null>(null);
  const [sub, setSub] = useState<string | null>(null);
  const navRef = useRef<HTMLElement>(null);

  useEffect(() => setSub(null), [open]);

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
                  {items.map((it, i) => (
                    <Entry key={i} item={it} sub={sub} setSub={setSub} close={() => setOpen(null)} />
                  ))}
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
      <span className="chip" title="Render credits on this device">
        Credits <b id="credits">{credits}</b>
      </span>
    </header>
  );
}
