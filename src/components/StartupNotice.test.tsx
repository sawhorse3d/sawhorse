// #115: while a start-up file waits for "Restore unsaved work?", the question is shown over
// the model area too, and after Restore the file's Open button is.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { noticeMessage, StartupNotice } from "./StartupNotice";
import type { StatusMessage } from "./ValueBar";

afterEach(cleanup);

const offer: StatusMessage = { text: "Restore unsaved work? 1 solid, 3 steps, saved just now. shelf.shz opens after you answer.", error: false, offer: true, actions: [{ label: "Restore", run: vi.fn() }, { label: "Discard", run: vi.fn() }] };
const rebuilt: StatusMessage = { ...offer, text: "rebuilt offer" };
const other: StatusMessage = { text: "Nothing to undo.", error: true };
const opened: StatusMessage = { text: "Restored your unsaved work. shelf.shz is not open yet.", error: false, startup: true, actions: [{ label: "Open shelf.shz", run: vi.fn() }] };

describe("what the notice over the model shows", () => {
  it("a file waiting and the offer on the status line: that very message", () => {
    expect(noticeMessage("shelf.shz", offer, () => rebuilt)).toBe(offer);
  });

  it("a file waiting and another message on the status line: the offer, made again", () => {
    expect(noticeMessage("shelf.shz", other, () => rebuilt)).toBe(rebuilt);
    expect(noticeMessage("shelf.shz", null, () => rebuilt)).toBe(rebuilt);
  });

  it("a file waiting and no offer left to make: nothing", () => {
    expect(noticeMessage("shelf.shz", other, () => null)).toBeNull();
  });

  it("no file waiting: the offer stays in the status line only", () => {
    expect(noticeMessage(null, offer, () => rebuilt)).toBeNull();
    expect(noticeMessage(null, null, () => rebuilt)).toBeNull();
  });

  it("no file waiting, after Restore with a file: the message with the Open button, while it is the status line's", () => {
    expect(noticeMessage(null, opened, () => rebuilt)).toBe(opened);
    expect(noticeMessage(null, other, () => rebuilt)).toBeNull();
  });
});

describe("every combination of a waiting file, the status line's message and an offer left", () => {
  it("a notice only while a file waits or right after Restore with one, and only ever the offer or that message", () => {
    // Before #115 nothing was shown over the model. Each of the 24 combinations (a file waiting
    // or not; six kinds of status line message; an offer left to make or not) is checked
    // against what may differ from that.
    const messages: (StatusMessage | null)[] = [null, offer, other, opened, { ...offer, error: true, actions: [offer.actions![1]] }, { text: "Unsaved work discarded.", error: false }];
    const counts = { none: 0, mirrored: 0, rebuilt: 0, opened: 0 };
    for (const waiting of ["shelf.shz", null])
    for (const message of messages)
    for (const left of [true, false]) {
      const got = noticeMessage(waiting, message, () => (left ? rebuilt : null));
      const why = JSON.stringify({ waiting, message: message?.text ?? null, left });
      if (got === null) {
        // Nothing over the model, as before: no file waits and no Open button, or no offer left.
        expect(waiting === null ? !message?.startup : !message?.offer && !left, why).toBe(true);
        counts.none++;
      } else if (waiting !== null) {
        // The status line's own offer when it shows one, else the offer made again.
        expect(got, why).toBe(message?.offer ? message : rebuilt);
        counts[got === rebuilt ? "rebuilt" : "mirrored"]++;
      } else {
        expect(got, why).toBe(message);
        expect(got.startup, why).toBe(true);
        counts.opened++;
      }
    }
    expect(counts).toEqual({ none: 14, mirrored: 4, rebuilt: 4, opened: 2 });
  });
});

describe("the notice", () => {
  it("shows the text and runs the message's own actions", () => {
    render(<StartupNotice message={offer} />);
    const n = screen.getByTestId("startup-notice");
    expect(n.textContent).toContain("shelf.shz opens after you answer.");
    const buttons = screen.getAllByRole("button");
    expect(buttons.map((b) => b.textContent)).toEqual(["Restore", "Discard"]);
    fireEvent.click(buttons[1]);
    expect(offer.actions![1].run).toHaveBeenCalledTimes(1);
    expect(offer.actions![0].run).not.toHaveBeenCalled();
  });

  it("an offer that cannot be restored reads as an error, as the status line does", () => {
    render(<StartupNotice message={{ ...offer, error: true, actions: [offer.actions![1]] }} />);
    expect(screen.getByTestId("startup-notice").className).toContain("err");
  });
});
