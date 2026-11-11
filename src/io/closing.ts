// Closing the desktop window over unsaved changes (lane 35, #44).
//
// Before lane 35 the window's X, Alt+F4 and every other close request closed the desktop
// app at once, and the unsaved changes were gone. The shell owns the close now
// (src-tauri/src/lib.rs, src-tauri/close-guard): the page tells it whether the model has
// unsaved changes ("unsaved" or "saved"); a close request over unsaved changes keeps the
// window open and sends `sawhorse://close-requested`; the page asks the question File > New
// asks, with "and close Sawhorse", and answers "keep" (No) or "close" (Yes).
//
// The window can never be held open by the page. The page does not register Tauri's own
// close-requested listener (with one, the shell holds every close until the page destroys
// the window). The shell closes the window on a close request two seconds after a question
// that has not been answered, whatever the page is doing, and every way the question can
// fail here (the dialog throwing, the handler throwing, a No sooner than a person could
// answer, which means the dialog never showed) says "failed", after which the next close
// request closes at once.
import { askYesNo } from "./files";

/** The event the shell sends when a close request finds unsaved changes. */
export const CLOSE_REQUESTED = "sawhorse://close-requested";

const DISCARD = "Discard the unsaved changes to this model";

/**
 * The question before unsaved changes are discarded, in one place: File > New, Open and
 * Close ask it as it is; closing the window (its X, Alt+F4, File > Exit) asks it with what
 * else happens, "and close Sawhorse".
 */
export function discardQuestion(closing = false): string {
  return `${DISCARD}${closing ? " and close Sawhorse" : ""}?`;
}

/** What the page says to the shell (the `close_guard` command; `sawhorse_close_guard::Say`). */
export type CloseWord = "unsaved" | "saved" | "keep" | "close" | "failed" | "exit";

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

/** A No answered sooner than this after the question went up was not a person's: the dialog never showed. */
export const ANSWER_FLOOR_MS = 150;
/** After Yes, the longest the window waits for the autosave to be deleted before it closes. */
export const DISCARD_WAIT_MS = 1000;

export interface CloseOptions {
  shell: CloseShell;
  /** Whether the model has unsaved changes now. */
  unsaved(): boolean;
  /** Yes is true. Default: askYesNo, the message dialog File > New uses. */
  ask?(question: string): Promise<boolean>;
  /** Yes was answered: do to the autosave what File > New does (delete it). */
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
  const ask = o.ask ?? askYesNo;
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
      const yes = await ask(discardQuestion(true));
      if (!yes) {
        if (now() - askedAt < ANSWER_FLOOR_MS) throw new Error("the question closed before it could be read");
        await o.shell.say("keep");
        return;
      }
      await Promise.race([o.discard().catch(() => undefined), new Promise((r) => setTimeout(r, DISCARD_WAIT_MS))]);
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
