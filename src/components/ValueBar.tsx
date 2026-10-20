import { forwardRef } from "react";
import { valueUnitKind, type ToolDef } from "../lib/tools";
import { unitInfo, type UnitSystem } from "../lib/units";

export interface ValueBarProps {
  tool: ToolDef;
  value: string;
  onChange: (v: string) => void;
  onCommit: (v: string) => void;
  onCancel: () => void;
  units: UnitSystem;
  onCycleUnits: () => void;
  /** Overrides the tool hint, e.g. to report a parse error or a kernel result. */
  message: StatusMessage | null;
}

/** A status-line message; `actions` render as buttons after the text ("Restore unsaved work?"). */
export interface StatusMessage {
  text: string;
  error: boolean;
  actions?: { label: string; run: () => void }[];
}

/** The Value Control Box: type mid-gesture, press Enter. */
export const ValueBar = forwardRef<HTMLInputElement, ValueBarProps>(function ValueBar(
  { tool, value, onChange, onCommit, onCancel, units, onCycleUnits, message },
  ref,
) {
  const info = unitInfo(units);
  // Lane 23b: a factor or a count has no unit, an angle is in degrees (lib/tools.ts).
  const kind = valueUnitKind(tool.vcb);
  return (
    <footer className="vcb">
      <div className={message?.error ? "hint err" : "hint"} id="hint" role="status" aria-live="polite">
        <b>{message?.error ? "Can't do that." : `${tool.label}.`}</b> {message ? message.text : tool.hint}
        {message?.actions?.map((a) => (
          <button key={a.label} type="button" className="hint-action" onClick={a.run}>
            {a.label}
          </button>
        ))}
      </div>
      {/* Always visible, as rendered in the approved mock (its `hidden` toggle never applied
          because .vcb label sets display:flex). Tools without a typed value show it read-only. */}
      <label id="vcbLabel" className={tool.vcb ? undefined : "idle"}>
        <span id="vcbName">{tool.vcb ?? "Measurements"}</span>{" "}
        <input
          id="vcb"
          ref={ref}
          value={value}
          readOnly={!tool.vcb}
          aria-label="Value"
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              onCommit(value);
            } else if (e.key === "Escape") {
              e.preventDefault();
              onCancel();
            }
          }}
        />{" "}
        {kind === "length" ? (
          <button
            type="button"
            className="units"
            title={`Units: ${info.label}. Click to change.`}
            onClick={(e) => {
              e.preventDefault();
              onCycleUnits();
            }}
          >
            {info.short}
          </button>
        ) : kind === "angle" ? (
          <span className="units unitless" data-testid="vcb-unit" title="Degrees">
            °
          </span>
        ) : (
          <span className="units unitless" data-testid="vcb-unit" title="A factor or a count: no unit" />
        )}
      </label>
    </footer>
  );
});
