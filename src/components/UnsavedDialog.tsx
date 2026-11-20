// The question over unsaved changes (#120), as it looks: the naming dialog's style, modal
// (src/components/Modal.tsx). Save, Discard Changes, Cancel, in that order. Save has the
// focus, so Enter saves; Escape is Cancel; Discard Changes is never the default: it takes a
// click, or Tab and then Enter. From the click on Save until the save has ended it shows
// what is being saved and offers Cancel alone, and the focus is on no button, so a second
// Enter does nothing. What each answer does is src/io/unsaved.ts.
import { useId } from "react";
import { UNSAVED_LABELS, type UnsavedPrompt } from "../io/unsaved";
import { Modal } from "./Modal";

export function UnsavedDialog({ prompt }: { prompt: UnsavedPrompt }) {
  const id = useId();
  return (
    <Modal className="name-dialog unsaved-dialog" labelledBy={id} onCancel={() => prompt.choose("cancel")}>
      <p id={id} className="question">
        {prompt.question}
      </p>
      {prompt.saving === null ? (
        <div className="btnrow">
          <button type="button" className="btn primary" data-autofocus="" onClick={() => prompt.choose("save")}>
            {UNSAVED_LABELS.save}
          </button>
          <button type="button" className="btn" onClick={() => prompt.choose("discard")}>
            {UNSAVED_LABELS.discard}
          </button>
          <button type="button" className="btn" onClick={() => prompt.choose("cancel")}>
            {UNSAVED_LABELS.cancel}
          </button>
        </div>
      ) : (
        <>
          <p className="saving" role="status">
            {prompt.saving}
          </p>
          <div className="btnrow">
            <button type="button" className="btn" onClick={() => prompt.choose("cancel")}>
              {UNSAVED_LABELS.cancel}
            </button>
          </div>
        </>
      )}
    </Modal>
  );
}
