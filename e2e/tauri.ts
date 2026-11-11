import type { Page } from "@playwright/test";

// Lane 35: the desktop app's bridge, stood in for in the browser build, so a spec can run the
// page as it runs inside the desktop shell (src/io/files.ts isTauri: `__TAURI_INTERNALS__`).
// It answers what the close question needs: the event plugin's listen and unlisten, the
// dialog plugin's message dialog (the question), and the shell's close_guard command, which
// it plays the way src-tauri/src/lib.rs does with sawhorse_close_guard (a close request over
// unsaved changes sends sawhorse://close-requested; "close" ends the window; "exit" is a
// close request). Every other command is refused, as a shell without it would.

/** How the stand-in's message dialog behaves. */
export type DialogMode = "answer" | "throw" | "throw-sync" | "never";

export interface TauriMock {
  /** Every command the page invoked, in order. */
  log: string[];
  /** Every word the page said through close_guard. */
  said: string[];
  /** Every message dialog's text. */
  asked: string[];
  /** The dialog's answer: "Ok" (yes) or "Cancel" (no), after `delay` ms. */
  answer: "Ok" | "Cancel";
  delay: number;
  dialog: DialogMode;
  /** close_guard words that fail (the shell not answering). */
  failWords: string[];
  /** The page's last word on unsaved changes, as the shell keeps it. */
  unsaved: boolean;
  /** How the window ended: "destroyed" (the page said close), "closed" (a request with nothing unsaved). */
  ended: string | null;
  /** A close request from the system (the X, Alt+F4), decided as the shell decides the first one. */
  request(): void;
  /** Send an event to the page's listeners. */
  emit(event: string, payload?: unknown): void;
  listeners(event: string): number;
}

/** Install the stand-in before the page's scripts run. */
export async function desktopBridge(page: Page, platform = "Win32"): Promise<void> {
  await page.addInitScript((platform: string) => {
    Object.defineProperty(Navigator.prototype, "platform", { get: () => platform });
    const w = window as unknown as Record<string, unknown>;
    const callbacks = new Map<number, (payload: unknown) => void>();
    const listeners = new Map<string, number[]>();
    let next = 1;
    const mock: TauriMock = {
      log: [],
      said: [],
      asked: [],
      answer: "Cancel",
      delay: 400,
      dialog: "answer",
      failWords: [],
      unsaved: false,
      ended: null,
      request() {
        if (mock.unsaved) mock.emit("sawhorse://close-requested", null);
        else mock.ended = "closed";
      },
      emit(event, payload = null) {
        for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id, payload });
      },
      listeners: (event) => (listeners.get(event) ?? []).length,
    };
    w.__tauriMock = mock;
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener(event: string, id: number) {
        listeners.set(
          event,
          (listeners.get(event) ?? []).filter((x) => x !== id),
        );
      },
    };
    w.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
      transformCallback(cb: (p: unknown) => void, once = false) {
        const id = next++;
        callbacks.set(id, (p) => {
          if (once) callbacks.delete(id);
          cb(p);
        });
        return id;
      },
      unregisterCallback(id: number) {
        callbacks.delete(id);
      },
      convertFileSrc: (p: string) => p,
      invoke(cmd: string, args: Record<string, unknown> = {}) {
        mock.log.push(cmd);
        if (cmd === "plugin:dialog|message" && mock.dialog === "throw-sync") throw new TypeError("the dialog plugin is not there");
        return (async () => {
          switch (cmd) {
            case "plugin:event|listen": {
              const event = String(args.event);
              listeners.set(event, [...(listeners.get(event) ?? []), Number(args.handler)]);
              return Number(args.handler);
            }
            case "plugin:event|unlisten":
              return null;
            case "plugin:dialog|message":
              mock.asked.push(String(args.message));
              if (mock.dialog === "throw") throw new Error("the dialog did not open");
              if (mock.dialog === "never") return new Promise(() => {});
              return new Promise((r) => setTimeout(() => r(mock.answer), mock.delay));
            case "close_guard": {
              const say = String(args.say);
              mock.said.push(say);
              if (mock.failWords.includes(say)) throw new Error(`close_guard ${say} did not reach the shell`);
              if (say === "unsaved") mock.unsaved = true;
              if (say === "saved") mock.unsaved = false;
              if (say === "close") mock.ended = "destroyed";
              if (say === "exit") mock.request();
              return null;
            }
            default:
              throw new Error(`${cmd} is not in this stand-in for the desktop shell`);
          }
        })();
      },
    };
  }, platform);
}

/** The stand-in's record, read from the page. */
export async function bridge(page: Page): Promise<Pick<TauriMock, "log" | "said" | "asked" | "unsaved" | "ended">> {
  return page.evaluate(() => {
    const m = (window as unknown as { __tauriMock: TauriMock }).__tauriMock;
    return { log: [...m.log], said: [...m.said], asked: [...m.asked], unsaved: m.unsaved, ended: m.ended };
  });
}

/** Change how the stand-in behaves. */
export async function setBridge(page: Page, over: Partial<Pick<TauriMock, "answer" | "delay" | "dialog" | "failWords">>): Promise<void> {
  await page.evaluate((o) => Object.assign((window as unknown as { __tauriMock: TauriMock }).__tauriMock, o), over);
}

/** A close request from the system (the window's X, Alt+F4). */
export async function closeWindow(page: Page): Promise<void> {
  await page.evaluate(() => (window as unknown as { __tauriMock: TauriMock }).__tauriMock.request());
}
