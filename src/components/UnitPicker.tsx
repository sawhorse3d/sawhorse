import { useEffect, useRef } from "react";
import { formatLength, UNIT_CHOICES, unitInfo, type UnitSystem } from "../lib/units";

/** The length every choice is shown with: 0.3 m, so "300 mm", "11.811 in", "11 13/16 in". */
export const SAMPLE_M = 0.3;

/** One line of the picker: the unit's name, and 0.3 m written in it. */
export function unitChoiceText(id: UnitSystem): { name: string; sample: string } {
  const info = unitInfo(id);
  const n = formatLength(SAMPLE_M, id).replace(/^~ /, "");
  // Feet and inches carry their own marks (11 13/16"); the others get the chip's label.
  return { name: info.name.replace(/^./, (c) => c.toUpperCase()), sample: id === "ft-in" ? n : `${n} ${info.short}` };
}

export interface UnitPickerProps {
  units: UnitSystem;
  onPick: (u: UnitSystem) => void;
  onClose: () => void;
}

/**
 * The unit picker (lane 24, issue #13): a small menu above the Value Control Box's unit chip
 * with the six unit systems, the current one checked, each with a sample length. Opened by
 * the chip, by the status line's "Units: ... Click to change." on an empty document, and
 * mirrored by Edit > Units. A pick closes it; so do Esc, a click outside, and Tab away.
 */
export function UnitPicker({ units, onPick, onClose }: UnitPickerProps) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    // Focus the current choice so Enter keeps it and the arrows move from it.
    const current = ref.current?.querySelector<HTMLButtonElement>('[aria-checked="true"]');
    (current ?? ref.current?.querySelector("button"))?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    };
    const onDown = (e: PointerEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    // Capture: the app's own key handler would read Esc as "cancel the gesture".
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("pointerdown", onDown);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("pointerdown", onDown);
    };
  }, [onClose]);
  const move = (from: number, by: number) => {
    const buttons = [...(ref.current?.querySelectorAll<HTMLButtonElement>("button[role=menuitemradio]") ?? [])];
    buttons[(from + by + buttons.length) % buttons.length]?.focus();
  };
  return (
    <div ref={ref} className="unit-picker" role="menu" aria-label="Units" data-testid="unit-picker">
      <b>Units</b>
      {UNIT_CHOICES.map((id, i) => {
        const { name, sample } = unitChoiceText(id);
        const checked = id === units;
        return (
          <button
            key={id}
            type="button"
            role="menuitemradio"
            aria-checked={checked}
            data-unit={id}
            onClick={() => onPick(id)}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                move(i, 1);
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                move(i, -1);
              }
            }}
          >
            <span>
              {checked ? "✓ " : "   "}
              {name}
            </span>
            <kbd>{sample}</kbd>
          </button>
        );
      })}
    </div>
  );
}
