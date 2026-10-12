// What has the keyboard (lane 7b). Ctrl+Z / Ctrl+Y and the other app shortcuts work while
// focus is on the panes' controls (a select, a checkbox, a button); only a field someone
// is typing into keeps them, because it has its own undo.

/** Input types that edit text (and so own Ctrl+Z). */
const TEXT_TYPES = new Set(["text", "search", "email", "url", "tel", "password", "number", ""]);

/** An editable text field: a text-like input, a textarea, or contenteditable; not read-only
 * or disabled. Selects, checkboxes, radios, ranges and colour pickers are not. */
export function isEditableText(el: EventTarget | null): boolean {
  if (typeof HTMLElement === "undefined" || !(el instanceof HTMLElement)) return false;
  if (el.isContentEditable) return true;
  if (el instanceof HTMLTextAreaElement) return !el.readOnly && !el.disabled;
  if (el instanceof HTMLInputElement) return TEXT_TYPES.has((el.getAttribute("type") ?? "").toLowerCase()) && !el.readOnly && !el.disabled;
  return false;
}

/** Keys a focused control that is not a text field uses itself: Space ticks a box, arrows move a range or a radio group, Enter presses. */
const CONTROL_KEYS = new Set([" ", "Enter", "Tab", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End", "PageUp", "PageDown"]);

/**
 * Whether a key pressed while `el` has focus is the control's rather than the app's (tool
 * keys, Esc, Delete, typing into the Value Control Box). A text field, a textarea and a
 * select own every key (a select picks an option by its first letter); a checkbox, radio,
 * range, colour picker or other input owns only the keys it uses, so a tool key pressed
 * after ticking a box in a pane (or a plugin's panel) still picks the tool.
 */
export function keyBelongsToControl(el: EventTarget | null, key: string): boolean {
  if (typeof HTMLElement === "undefined" || !(el instanceof HTMLElement)) return false;
  if (el.isContentEditable || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) return true;
  if (el instanceof HTMLInputElement) return TEXT_TYPES.has((el.getAttribute("type") ?? "").toLowerCase()) || CONTROL_KEYS.has(key);
  return false;
}
