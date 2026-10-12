// Our own tool glyphs, drawn for the approved mock (ADR-0003: own glyphs, no copied icons).
// 24x24 viewBox, stroked with currentColor by the .tool svg rule.
import type { ReactElement } from "react";
import type { BuiltinToolId } from "../lib/tools";

export const TOOL_GLYPHS: Record<BuiltinToolId, ReactElement> = {
  select: <path d="M5 3l14 9-6 1-3 6z" />,
  erase: <path d="M4 16l9-9 6 6-9 9H8z M13 7l4-4" />,
  line: (
    <>
      <path d="M4 20L20 4" />
      <circle cx="4" cy="20" r="1.5" />
      <circle cx="20" cy="4" r="1.5" />
    </>
  ),
  rect: <rect x="4" y="6" width="16" height="12" />,
  circle: <circle cx="12" cy="12" r="8" />,
  arc: <path d="M4 18a8 8 0 0 1 16 0" />,
  pushpull: (
    <>
      <path d="M4 16l6 4 10-6V8l-6-4L4 10z M10 20V10l10-6 M10 10L4 6" />
      <path d="M14 3v5M12 6l2-2 2 2" />
    </>
  ),
  move: <path d="M12 3v18M3 12h18M12 3l-3 3M12 3l3 3M12 21l-3-3M12 21l3-3M3 12l3-3M3 12l3 3M21 12l-3-3M21 12l-3 3" />,
  rotate: (
    <>
      <path d="M20 12a8 8 0 1 1-8-8" />
      <path d="M12 4l3-2M12 4l3 2" />
    </>
  ),
  scale: (
    <>
      <rect x="4" y="10" width="10" height="10" />
      <path d="M14 10l6-6M20 4h-5M20 4v5" />
    </>
  ),
  offset: (
    <>
      <rect x="3" y="3" width="18" height="18" />
      <rect x="8" y="8" width="8" height="8" />
    </>
  ),
  follow: (
    <>
      <path d="M3 18c5 0 5-12 10-12s5 12 8 12" />
      <circle cx="3" cy="18" r="2" />
    </>
  ),
  tape: <path d="M3 15l12-12 6 6-12 12z M7 11l2 2M10 8l2 2M13 5l2 2" />,
  dim: <path d="M3 12h18M3 8v8M21 8v8" />,
  paint: <path d="M4 20c0-3 2-5 5-5l6-9 4 4-9 6c0 3-2 5-6 4z M12 8l4 4" />,
  // Lane 12: a plane seen at an angle, with the arrows that show the side it keeps.
  section: <path d="M5 8l10-4v12L5 20z M15 10h5 M18 8l2 2-2 2 M10 9v6" />,
};

export function BrandMark() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M3 9h18M5 9l-2 11M19 9l2 11M8 9l-1.5 11M16 9l1.5 11M6.5 15h11"
        stroke="currentColor"
        strokeWidth="1.8"
        fill="none"
        strokeLinecap="round"
      />
    </svg>
  );
}

/** Small panel glyphs (Outliner, Tags, scene tabs): 16x16, stroked with currentColor. */
export function PanelGlyph({ name }: { name: "eye" | "eye-off" | "lock" | "unlock" | "open" | "closed" | "plus" | "tag" | "trash" }) {
  const d: Record<typeof name, string> = {
    eye: "M1.5 8s2.5-4.5 6.5-4.5S14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8z M8 6.2a1.8 1.8 0 1 0 0 3.6 1.8 1.8 0 0 0 0-3.6z",
    "eye-off": "M1.5 8s2.5-4.5 6.5-4.5S14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8z M2.5 13.5l11-11",
    lock: "M4 7.5h8v6H4z M5.5 7.5V5.2a2.5 2.5 0 0 1 5 0v2.3",
    unlock: "M4 7.5h8v6H4z M5.5 7.5V5.2a2.5 2.5 0 0 1 4.8-1",
    open: "M4 6l4 4 4-4",
    closed: "M6 4l4 4-4 4",
    plus: "M8 3v10M3 8h10",
    tag: "M2.5 2.5h5.5l5.5 5.5-5.5 5.5-5.5-5.5z M5.2 5.2h.1",
    trash: "M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.5h5.8l.6-8.5",
  };
  return (
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" className="pg">
      <path d={d[name]} stroke="currentColor" strokeWidth="1.3" fill="none" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

/** A plugin tool without its own icon: a puzzle piece (ours). */
export const PLUGIN_GLYPH = <path d="M5 9h3a2 2 0 1 1 4 0h3v3a2 2 0 1 1 0 4v3h-3a2 2 0 1 0-4 0H5v-3a2 2 0 1 0 0-4z" />;
