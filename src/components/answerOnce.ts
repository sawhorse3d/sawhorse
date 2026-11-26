// #145: the click handler for a button that answers a question whose next question takes its
// place at once ("Restore unsaved work?" with more than one set kept). A second click or key
// press meant for the first must not answer the next, which the person has not read:
// - the second click of a double-click (a click whose `detail` is 2 or more) answers nothing:
//   the next question's button may be drawn under the pointer, and the second click land on it;
// - the button that answered gives up the focus: the next question's button of the same name is
//   the same element (the buttons are keyed by label), so it would keep the focus, and Enter
//   pressed again or held would press it.
// A key press is a click with `detail` 0, so a key still answers.
import type { MouseEvent } from "react";

export function answerOnce(run: () => void) {
  return (e: MouseEvent<HTMLButtonElement>) => {
    if (e.detail >= 2) return;
    e.currentTarget.blur();
    run();
  };
}
