// Our own tool glyphs, drawn for the approved mock (ADR-0003: own glyphs, no copied icons).
// 24x24 viewBox, stroked with currentColor by the .tool svg rule.
import type { ReactElement } from "react";
import type { ToolId } from "../lib/tools";

export const TOOL_GLYPHS: Record<ToolId, ReactElement> = {
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
