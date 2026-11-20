// Closing the desktop window over unsaved changes (lane 35, #44).
//
// Before lane 35 the window's X, Alt+F4 and every other close request closed the desktop
// app at once, and the unsaved changes were gone. The shell owns the close now
// (src-tauri/src/lib.rs, src-tauri/close-guard): the page tells it whether the model has
// unsaved changes ("unsaved" or "saved"); a close request over unsaved changes keeps the
// window open and sends `sawhorse://close-requested`; the page asks the question File > New
// asks (src/io/unsaved.ts, #120), with "before Sawhorse closes", and answers "keep" (Cancel,
// or a save that was cancelled or failed) or "close" (Discard Changes, or Save once the
// model is saved). Discard Changes deletes this session's autosave, as File > New does;
// unsaved work set aside at start and not yet answered (lane 64, #94) is not this session's
// and stays for the next start.
//
// The window can never be held open by the page. The page does not register Tauri's own
// close-requested listener (with one, the shell holds every close until the page destroys
// the window). The shell closes the window on a close request two seconds after a question
// that has not been answered, whatever the page is doing, and every way the question can
// fail here (the dialog throwing, the handler throwing, a Cancel sooner than a person could
// answer, which means the dialog did not stay up) says "failed", after which the next close
// request closes at once. A save chosen at the question can take longer than two seconds
// (a file dialog): while it runs the page says "saving" to the shell several times a second
// (src/io/unsaved.ts, SAVING_BEAT_MS), and the two seconds count from the last one heard.
import type { UnsavedOutcome } from "./unsaved";

/** The event the shell sends when a close request finds unsaved changes. */
export const CLOSE_REQUESTED = "sawhorse://close-requested";

/** What the page says to the shell (the `close_guard` command; `sawhorse_close_guard::Say`). */
export type CloseWord = "unsaved" | "saved" | "keep" | "close" | "saving" | "failed" | "exit";

/** The shell, as the page sees it. */
export interface CloseShell {
  say(word: CloseWord): Promise<void>;
  /** Call `cb` on each `sawhorse://close-requested`; resolves to the function that stops. */
  onRequest(cb: () => void): Promise<() => void>;
}

/** The desktop app's shell. */
export function desktopShell(): CloseShell {
  return {
    async say(word) {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("close_guard", { say: word });
    },
    async onRequest(cb) {
      const { listen } = await import("@tauri-apps/api/event");
      return listen(CLOSE_REQUESTED, () => cb());
    },
  };
}

/** A Cancel sooner than this after the question went up was not a person's: the dialog did not stay up. */
export const ANSWER_FLOOR_MS = 150;
/** After Discard Changes, the longest the window waits for the autosave to be deleted before it closes. */
export const DISCARD_WAIT_MS = 1000;

export interface CloseOptions {
  shell: CloseShell;
  /** Whether the model has unsaved changes now. */
  unsaved(): boolean;
  /** Ask the question File > New asks, with "before Sawhorse closes" (src/io/unsaved.ts), and run the save when Save is the answer. */
  ask(): Promise<UnsavedOutcome>;
  /** Discard Changes was answered: do to the autosave what File > New does (delete it). */
  discard(): Promise<void>;
  /** The question could not be asked: say so (the next close closes without asking). */
  failed(message: string): void;
  /** Milliseconds, for the answer floor. Default: performance.now. */
  now?(): number;
}

/**
 * The page's answer to `sawhorse://close-requested`. One question at a time: a request that
 * arrives while the question is up is ignored here (the shell sends none within two seconds,
 * and after that it closes the window without waiting).
 */
export function closeHandler(o: CloseOptions): () => Promise<void> {
  const now = o.now ?? (() => performance.now());
  let asking = false;
  return async () => {
    if (asking) return;
    asking = true;
    try {
      // Saved since the shell looked: nothing to ask.
      if (!o.unsaved()) {
        await o.shell.say("close");
        return;
      }
      const askedAt = now();
      const answer = await o.ask();
      if (answer === "cancelled") {
        if (now() - askedAt < ANSWER_FLOOR_MS) throw new Error("the question closed before it could be read");
        await o.shell.say("keep");
        return;
      }
      // Save, and the save was cancelled or failed (the status line says why): as Cancel.
      if (answer === "not-saved") {
        await o.shell.say("keep");
        return;
      }
      // Saved (the save deleted the autosave, as File > Save does) or nothing left unsaved:
      // only Discard Changes has an autosave to delete.
      if (answer === "discarded") await Promise.race([o.discard().catch(() => undefined), new Promise((r) => setTimeout(r, DISCARD_WAIT_MS))]);
      await o.shell.say("close");
    } catch (e) {
      const why = e instanceof Error ? e.message : String(e);
      try {
        o.failed(`Sawhorse could not ask about the unsaved changes (${why}). Close the window again to close it without saving, or save first.`);
      } catch {
        /* the shell is told below either way */
      }
      await o.shell.say("failed").catch(() => undefined);
    } finally {
      asking = false;
    }
  };
}
