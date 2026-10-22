import { forwardRef, useCallback, useEffect, useState } from "react";
import { valueUnitKind, type ToolDef } from "../lib/tools";
import { unitInfo, type UnitSystem } from "../lib/units";
import { UnitPicker } from "./UnitPicker";
import { dismissUnitsHint, UnitsHint, unitsHintDismissed } from "./UnitsHint";

export interface ValueBarProps {
  tool: ToolDef;
  value: string;
  onChange: (v: string) => void;
  onCommit: (v: string) => void;
  onCancel: () => void;
  units: UnitSystem;
  /** The unit picker's choice (lane 24; the chip used to cycle). */
  onUnits: (u: UnitSystem) => void;
  /** Overrides the tool hint, e.g. to report a parse error or a kernel result. */
  message: StatusMessage | null;
  /** True while nothing is drawn: the status line then names the units and offers the picker. */
  empty?: boolean;
  /** Lane 24: the browser demo's first-run hint about units (never in the desktop app). */
  firstRunHint?: boolean;
  /** Lane 24: Edit > Units opens the same picker; a change of this number opens it. */
  openPicker?: number;
}

/** A status-line message; `actions` render as buttons after the text ("Restore unsaved work?"). */
export interface StatusMessage {
  text: string;
  error: boolean;
  actions?: { label: string; run: () => void }[];
}

/** The Value Control Box: type mid-gesture, press Enter. */
export const ValueBar = forwardRef<HTMLInputElement, ValueBarProps>(function ValueBar(
  { tool, value, onChange, onCommit, onCancel, units, onUnits, message, empty = false, firstRunHint = false, openPicker = 0 },
  ref,
) {
  const info = unitInfo(units);
  // Lane 23b: a factor or a count has no unit, an angle is in degrees (lib/tools.ts).
  const kind = valueUnitKind(tool.vcb);
  const [picking, setPicking] = useState(false);
  const [hint, setHint] = useState(() => firstRunHint && !unitsHintDismissed());
  const dismiss = useCallback(() => {
    setHint(false);
    dismissUnitsHint();
  }, []);
  const open = useCallback(() => {
    setPicking(true);
    // Opening the picker is finding the units: the hint has done its job.
    setHint((h) => {
      if (h) dismissUnitsHint();
      return false;
    });
  }, []);
  const close = useCallback(() => setPicking(false), []);
  useEffect(() => {
    if (openPicker > 0) open();
  }, [openPicker, open]);
  const pick = (u: UnitSystem) => {
    setPicking(false);
    onUnits(u);
  };
  // The status line names the units while the document is empty: a first look at the app
  // finds them where the eye lands (lane 24, issue #13), and the sentence is the click.
  const unitsLine = empty && !message;
  return (
    <footer className="vcb">
      <div className={message?.error ? "hint err" : "hint"} id="hint" role="status" aria-live="polite">
        <b>{message?.error ? "Can't do that." : `${tool.label}.`}</b> {message ? message.text : tool.hint}
        {message?.actions?.map((a) => (
          <button key={a.label} type="button" className="hint-action" onClick={a.run}>
            {a.label}
          </button>
        ))}
        {unitsLine ? (
          <button type="button" className="hint-action hint-units" data-testid="hint-units" title="Choose the units lengths are typed and shown in" onClick={open}>
            Units: {info.name}. Click to change.
          </button>
        ) : null}
      </div>
      {/* Always visible, as rendered in the approved mock (its `hidden` toggle never applied
          because .vcb label sets display:flex). Tools without a typed value show it read-only. */}
      <div className="vcb-box">
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
            title={`Units: ${info.name}. Click to change.`}
            aria-label={`Units: ${info.name}. Click to change.`}
            aria-haspopup="menu"
            aria-expanded={picking}
            onClick={(e) => {
              e.preventDefault();
              if (picking) close();
              else open();
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
      {/* Outside the label: a click on them must not land in the input. */}
      {picking ? <UnitPicker units={units} onPick={pick} onClose={close} /> : null}
      {hint && !picking ? <UnitsHint onDismiss={dismiss} /> : null}
      </div>
    </footer>
  );
});
