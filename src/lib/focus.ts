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
