// The question over unsaved changes (#120): Save, Discard Changes, Cancel.
//
// Before #120 every place that was about to replace or close a model with unsaved changes
// asked "Discard the unsaved changes to this model?" with OK and Cancel, through the
// desktop's message dialog or the browser's window.confirm: to keep the work a person
// answered Cancel, saved, and did the thing again. Now the question is the app's own dialog
// (src/components/UnsavedDialog.tsx), the same in both builds, with three answers:
//
// - Save saves as File > Save does (asking for a name when the model is untitled) and then
//   carries on with what was asked. A save that is cancelled or fails leaves everything as
//   it was, as Cancel does; a failed one says why on the status line, as File > Save does.
// - Discard Changes does what OK did.
// - Cancel does what Cancel did. Escape is Cancel.
//
// This file is the question without its looks: what is asked, one question at a time, what
// each answer does, and the word the desktop shell is told while a save chosen here runs
// ("saving", src/io/closing.ts; src-tauri/close-guard). It draws nothing and knows nothing
// of React; App.tsx gives it the dialog to show and the save to run.

/** The three answers. */
export type UnsavedChoice = "save" | "discard" | "cancel";

/** How a save ended (App.tsx, `save`). */
export type SaveResult = "saved" | "cancelled" | "failed";

/**
 * How the question ended.
 * - "clean": nothing was unsaved by the time it would have been asked; nothing was asked.
 * - "saved": Save, and the model is saved.
 * - "discarded": Discard Changes.
 * - "cancelled": Cancel or Escape.
 * - "replaced": another question took this one's place before it was answered; nothing
 *   happens, as with Cancel.
 * - "not-saved": Save, and the save was cancelled or failed, or was given up on.
 */
export type UnsavedOutcome = "clean" | "saved" | "discarded" | "cancelled" | "replaced" | "not-saved";

/** Whether what was asked for (New, Open, Close, closing the window) goes ahead. */
export function goesAhead(o: UnsavedOutcome): boolean {
  return o === "clean" || o === "saved" || o === "discarded";
}

/**
 * How often the shell is told "saving" while a save chosen at the question runs. The shell
 * closes the window on a close request two seconds after the last one it heard
 * (`sawhorse_close_guard::PATIENCE_MS`); a timer in a window that is covered, or behind a
 * native dialog, can be slowed to once a second, which eight a second leaves room for.
 */
export const SAVING_BEAT_MS = 250;

/** The question's words: the model by its file's name, and what else happens at a close. */
export function unsavedQuestionText(name: string | null, closing = false): string {
  return `Save the changes to ${name ?? "this model"}${closing ? " before Sawhorse closes" : ""}?`;
}

/** The labels, in the order they are shown. Save has the focus; Discard Changes is never the default. */
export const UNSAVED_LABELS: Record<UnsavedChoice, string> = { save: "Save", discard: "Discard Changes", cancel: "Cancel" };

/** What the dialog shows. */
export interface UnsavedPrompt {
  question: string;
  /** null while the three answers are offered; the line shown from the click on Save until the save has ended, when only Cancel is offered. */
  saving: string | null;
  /** An answer. While saving, only "cancel" does anything: it gives up on the save. */
  choose(c: UnsavedChoice): void;
}

export interface AskOptions {
  /** The window is closing: the question says so. */
  closing?: boolean;
  /** Asked from inside the app's edit queue (an empty SketchUp file, Restore): the save must not wait for the queue it is in. */
  queued?: boolean;
  /** Ask whatever `unsaved()` says (Restore, where the model's log says there is work that cannot be set aside). */
  always?: boolean;
}

export interface UnsavedOptions {
  /** Whether the model has unsaved changes now. */
  unsaved(): boolean;
  /** The model's file name, or null when it is untitled. */
  name(): string | null;
  /** Show the dialog, or take it away (null). */
  show(p: UnsavedPrompt | null): void;
  /** File > Save. Never rejects in App.tsx; a rejection here counts as "failed". */
  save(o: { signal: AbortSignal; queued: boolean }): Promise<SaveResult>;
  /** Called at once and then every SAVING_BEAT_MS while a save chosen here runs (the desktop app: "saving" to the shell). */
  beat?(): void;
  /** Call `fn` every `ms`; returns the function that stops. Default: setInterval. */
  every?(fn: () => void, ms: number): () => void;
}

export interface UnsavedQuestion {
  /** Ask, if there is anything unsaved. */
  ask(a?: AskOptions): Promise<UnsavedOutcome>;
  /** Whether a question is up (answers offered, or a save running). */
  up(): boolean;
}

export function unsavedQuestion(o: UnsavedOptions): UnsavedQuestion {
  const every =
    o.every ??
    ((fn: () => void, ms: number) => {
      const id = setInterval(fn, ms);
      return () => clearInterval(id);
    });
  /** The question that is up. */
  let current: { settle(out: UnsavedOutcome): void; saving: Promise<void> | null } | null = null;

  async function ask(a: AskOptions = {}): Promise<UnsavedOutcome> {
    // A save chosen at the question before this one is running, perhaps with its file dialog
    // up: nothing is asked over it. How it ends decides whether there is anything to ask.
    // A question from inside the edit queue does not wait for it: that save was asked for
    // outside the queue and waits for the queue to empty, so each would wait for the other
    // for good. The model stays as it is, as with Cancel.
    while (current?.saving) {
      if (a.queued) return "cancelled";
      await current.saving;
    }
    if (!a.always && !o.unsaved()) return "clean";
    // One question at a time. The one showing has not been answered, and what is asked for
    // now (a close request, a file dropped on the window) replaces what it was asked for:
    // it ends as Cancel does ("replaced": no person answered it, however soon it ended).
    current?.settle("replaced");
    return new Promise<UnsavedOutcome>((resolve) => {
      const name = o.name();
      const question = unsavedQuestionText(name, a.closing);
      let done = false;
      let abort: AbortController | null = null;
      let stopBeat = () => {};
      const me: NonNullable<typeof current> = {
        saving: null,
        settle(out) {
          if (done) return;
          done = true;
          stopBeat();
          if (current === me) {
            current = null;
            try {
              o.show(null);
            } catch {
              /* the answer stands whether or not the dialog could be taken away */
            }
          }
          resolve(out);
        },
      };
      const choose = (c: UnsavedChoice) => {
        if (done) return;
        if (me.saving) {
          // Only Cancel is offered while saving: it gives up on the save. What can still be
          // stopped is (nothing is written after the signal), and the shell is told no more.
          if (c !== "cancel") return;
          abort?.abort();
          me.settle("not-saved");
          return;
        }
        if (c === "cancel") return me.settle("cancelled");
        if (c === "discard") return me.settle("discarded");
        const signal = (abort = new AbortController()).signal;
        const beat = () => {
          try {
            o.beat?.();
          } catch {
            /* the save goes on whether or not the shell hears of it */
          }
        };
        beat();
        stopBeat = every(beat, SAVING_BEAT_MS);
        me.saving = (async () => {
          let r: SaveResult = "failed";
          try {
            r = await o.save({ signal, queued: !!a.queued });
          } catch {
            r = "failed";
          }
          // Saved, and nothing changed while it was saved (a file dropped on the window, a
          // plugin's batch): what was asked for goes ahead. A change the file does not hold
          // stays unsaved, so nothing goes ahead over it.
          me.settle(r === "saved" && !o.unsaved() ? "saved" : "not-saved");
        })();
        o.show({ question, saving: name ? `Saving ${name}…` : "Saving…", choose });
      };
      current = me;
      try {
        o.show({ question, saving: null, choose });
      } catch (e) {
        // The dialog could not be shown: nothing was asked, and the caller hears why.
        done = true;
        current = null;
        throw e;
      }
    });
  }

  return { ask, up: () => current !== null };
}
