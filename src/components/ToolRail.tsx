import { TOOL_GROUPS, type BuiltinToolId, type ToolDef, type ToolId } from "../lib/tools";
import { PLUGIN_GLYPH, TOOL_GLYPHS } from "./glyphs";

export interface ToolRailProps {
  active: ToolId;
  onSelect: (id: ToolId) => void;
  /** Tools added by plugins (lane 11): their own group at the end of the rail. */
  plugins?: ToolDef[];
}

function glyph(t: ToolDef) {
  if (t.plugin) return t.icon ? <path d={t.icon} /> : PLUGIN_GLYPH;
  return TOOL_GLYPHS[t.id as BuiltinToolId];
}

export function ToolRail({ active, onSelect, plugins = [] }: ToolRailProps) {
  const groups = plugins.length ? [...TOOL_GROUPS, plugins] : TOOL_GROUPS;
  return (
    <aside className="rail" aria-label="Tools">
      {groups.map((group, gi) => (
        <div className="grp" key={gi} data-group={gi === TOOL_GROUPS.length ? "plugins" : undefined} aria-label={gi === TOOL_GROUPS.length ? "Plugin tools" : undefined}>
          {group.map((t) => (
            <button
              key={t.id}
              type="button"
              className="tool"
              data-tool={t.id}
              aria-pressed={active === t.id}
              aria-keyshortcuts={t.plugin ? undefined : t.key === " " ? "Space" : t.keyCap || undefined}
              title={t.plugin ? `${t.label} (${t.plugin})${t.keyCap ? ` ${t.keyCap}` : ""}` : t.key ? `${t.label} (${t.key === " " ? "Space" : t.keyCap})` : t.label}
              onClick={() => onSelect(t.id)}
            >
              {/* Lane 12's Section Plane has no key: no key cap. */}
              {t.plugin || t.keyCap ? <kbd>{t.plugin ? "" : t.keyCap}</kbd> : null}
              <svg viewBox="0 0 24 24" aria-hidden="true">
                {glyph(t)}
              </svg>
              <small>{t.label}</small>
            </button>
          ))}
        </div>
      ))}
    </aside>
  );
}
