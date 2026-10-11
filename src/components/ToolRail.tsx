import { TOOL_GROUPS, type ToolId } from "../lib/tools";
import { TOOL_GLYPHS } from "./glyphs";

export interface ToolRailProps {
  active: ToolId;
  onSelect: (id: ToolId) => void;
}

export function ToolRail({ active, onSelect }: ToolRailProps) {
  return (
    <aside className="rail" aria-label="Tools">
      {TOOL_GROUPS.map((group, gi) => (
        <div className="grp" key={gi}>
          {group.map((t) => (
            <button
              key={t.id}
              type="button"
              className="tool"
              data-tool={t.id}
              aria-pressed={active === t.id}
              aria-keyshortcuts={t.key === " " ? "Space" : t.keyCap}
              title={`${t.label} (${t.key === " " ? "Space" : t.keyCap})`}
              onClick={() => onSelect(t.id)}
            >
              <kbd>{t.keyCap}</kbd>
              <svg viewBox="0 0 24 24" aria-hidden="true">
                {TOOL_GLYPHS[t.id]}
              </svg>
              <small>{t.label}</small>
            </button>
          ))}
        </div>
      ))}
    </aside>
  );
}
