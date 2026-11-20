// A real modal (#120): the question over unsaved changes, and the naming dialog.
//
// It is the platform's own modal, a <dialog> opened with showModal(): everything outside it
// takes no clicks, no focus and no keys while it is up, and Tab stays inside it. On top of
// what the element does by itself:
//
// - `role="dialog"` and `aria-modal="true"` are written out, with a name (`label`, or the id
//   of the element that holds it, `labelledBy`).
// - The focus goes to the element marked `data-autofocus`, or to the dialog itself, when it
//   opens. It is held inside: if what had it goes away (a button that is replaced), the
//   dialog takes it, so a key never lands on the page behind.
// - The focus is put back where it was when the dialog goes, if that is still there.
// - Escape is `onCancel`, and so is any other way the platform closes a dialog.
// - No key pressed while it is up reaches the app's own shortcuts (tool keys, Ctrl+S,
//   Delete): keys inside stop at the dialog, and a key that lands outside it is stopped
//   before the app hears it.
import { useEffect, useLayoutEffect, useRef, type ReactNode } from "react";

export interface ModalProps {
  /** The dialog's name, for a screen reader. */
  label?: string;
  /** Or the id of the element inside that names it. */
  labelledBy?: string;
  className?: string;
  /** Escape, or the platform closing the dialog. */
  onCancel: () => void;
  children: ReactNode;
}

export function Modal({ label, labelledBy, className, onCancel, children }: ModalProps) {
  const ref = useRef<HTMLDialogElement>(null);
  const cancel = useRef(onCancel);
  cancel.current = onCancel;

  useLayoutEffect(() => {
    const d = ref.current;
    if (!d) return;
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    // A test's document (jsdom) may have no showModal: the dialog is shown, not modal, there.
    if (!d.open) {
      if (typeof d.showModal === "function") d.showModal();
      else d.setAttribute("open", "");
    }
    (d.querySelector<HTMLElement>("[data-autofocus]") ?? d).focus();
    // A key that lands outside the dialog (the focus was lost to the page) is not the app's.
    const outside = (e: KeyboardEvent) => {
      if (e.target instanceof Node && d.contains(e.target)) return;
      e.stopPropagation();
      if (e.type === "keydown" && e.key === "Escape") cancel.current();
    };
    window.addEventListener("keydown", outside, true);
    window.addEventListener("keyup", outside, true);
    // Closed by the platform, not by us taking it away (a second Escape that a browser does
    // not let a page refuse): that is Cancel.
    let ours = false;
    const closed = () => {
      if (!ours) cancel.current();
    };
    d.addEventListener("close", closed);
    return () => {
      ours = true;
      window.removeEventListener("keydown", outside, true);
      window.removeEventListener("keyup", outside, true);
      d.removeEventListener("close", closed);
      if (d.open) {
        if (typeof d.close === "function") d.close();
        else d.removeAttribute("open");
      }
      if (before?.isConnected) before.focus();
    };
  }, []);

  // Held inside: after any render that took away what had the focus.
  useEffect(() => {
    const d = ref.current;
    if (d && !d.contains(document.activeElement)) d.focus();
  });

  return (
    <dialog
      ref={ref}
      className={className}
      role="dialog"
      aria-modal="true"
      aria-label={label}
      aria-labelledby={labelledBy}
      tabIndex={-1}
      onCancel={(e) => {
        e.preventDefault();
        onCancel();
      }}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape") {
          e.preventDefault();
          onCancel();
        }
      }}
      onKeyUp={(e) => e.stopPropagation()}
    >
      {children}
    </dialog>
  );
}
