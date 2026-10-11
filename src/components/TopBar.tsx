import { BrandMark } from "./glyphs";
import { VIEWS, type ViewId } from "../viewport/engine";

const MENUS = ["File", "Edit", "View", "Draw", "Tools", "Window", "Help"];

export interface TopBarProps {
  view: ViewId;
  onView: (v: ViewId) => void;
  credits: number;
}

export function TopBar({ view, onView, credits }: TopBarProps) {
  return (
    <header className="top">
      <div className="brand" title="Sawhorse">
        <BrandMark />
        Sawhorse
      </div>
      <nav className="menu" aria-label="Menu">
        {MENUS.map((m) => (
          <button key={m} type="button">
            {m}
          </button>
        ))}
      </nav>
      <div className="spacer"></div>
      <div className="views" role="group" aria-label="Views">
        {VIEWS.map((v) => (
          <button key={v.id} type="button" aria-pressed={view === v.id} onClick={() => onView(v.id)}>
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
