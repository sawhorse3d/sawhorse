// What a tool says when it drops a click or a drag (#130). A click can find no point under
// the cursor: in the Front or Right view of an empty model the eye ray meets no ground and no
// face, and nothing else is there to snap to. A press and release far enough
// apart is a drag, which the click, move, click tools do not take. Either way nothing is
// drawn and nothing changes; these are the words the status line shows instead of silence.
//
// Halfway through a gesture (a first corner placed) the words end with what the next click
// does, so the hint the person needs is still on the status line and the gesture stays as
// it was.

/** The advice after the words: where a click finds a point. */
const FIND_A_POINT = "click the ground or a face, or use Top or Iso.";

/**
 * A click with no point under it. `what` is what the tool would have done ("draw on",
 * "measure from"); `step` is the hint for the next click halfway through a gesture (null
 * before the first click).
 */
export function noPointWords(what: string, step: string | null): string {
  return step ? `Nothing to click on there. ${step}` : `Nothing to ${what} there: ${FIND_A_POINT}`;
}

/**
 * A drag, dropped. `verb` is what a drag does not do ("draw", "measure"); `how` says how the
 * tool takes its points; `step` as in `noPointWords`.
 */
export function dragWords(verb: string, how: string, step: string | null): string {
  return step ? `A drag does not ${verb}. ${step}` : `${how} A drag does not ${verb}.`;
}

/** The tools that take points by clicking: click, move, then click again. */
export const CLICK_MOVE_CLICK = "Click, move, then click again.";
