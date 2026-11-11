import type { Page } from "@playwright/test";

// Lane 35: the desktop app's bridge, stood in for in the browser build, so a spec can run the
// page as it runs inside the desktop shell (src/io/files.ts isTauri: `__TAURI_INTERNALS__`).
// It answers what the close question needs: the event plugin's listen and unlisten, the
// dialog plugin's message dialog (the question), and the shell's close_guard command, which
// it plays the way src-tauri/src/lib.rs does with sawhorse_close_guard (a close request over
// unsaved changes sends sawhorse://close-requested; "close" ends the window; "exit" is a
// close request). Every other command is refused, as a shell without it would.
//
// Lane 37 (#45): the file the app was started with. The shell's start_file command is played
// as lib.rs plays it: the first call adds the path to the fs scope and returns it, every later
// call returns null. The shell outlives a reload of its page, so what it has handed over is
// kept in the tab's sessionStorage, and restart() plays a new start of the app with another
// file (the page's IndexedDB, where the autosave lives, is kept, as it is between two runs
// of the desktop app). The fs plugin's stat and read_file read the files given to
// desktopBridge, and only paths in the scope, as the real scope allows: a dropped path
// (drop()) or the start-up file.
//
// Lane 39: the window's title. The shell's window_title command is recorded with the text the
// page sent (`titles`); the shell's cleaning of it is tested in src-tauri/window-title.
// `titleFails` makes the command throw, as a shell without it would.

/** How the stand-in's message dialog behaves. */
export type DialogMode = "answer" | "throw" | "throw-sync" | "never";

/** The files on the stand-in's disk: base64 bytes, or null for a file that cannot be read. */
export type DesktopFiles = Record<string, string | null>;

/** The desktop app's start: the file it was started with, and the files on its disk. */
export interface DesktopStart {
  start?: string | null;
  files?: DesktopFiles;
}

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
  /** Every path the shell added to the fs scope, in order (this page load). */
  allowed: string[];
  /** Every path the fs plugin was asked to read (stat or read_file), in order (this page load). */
  reads: string[];
  /** Every title the page sent through window_title, in order (this page load). */
  titles: string[];
  /** window_title throws instead of answering. */
  titleFails: boolean;
  /** Files dropped on the window: allowed in the scope, then sawhorse://dropped, as lib.rs does. */
  drop(paths: string[]): void;
}

/** Install the stand-in before the page's scripts run. */
export async function desktopBridge(page: Page, platform = "Win32", start: DesktopStart = {}): Promise<void> {
  await page.addInitScript(({ platform, start }: { platform: string; start: DesktopStart }) => {
    Object.defineProperty(Navigator.prototype, "platform", { get: () => platform });
    const w = window as unknown as Record<string, unknown>;
    const callbacks = new Map<number, (payload: unknown) => void>();
    const listeners = new Map<string, number[]>();
    let next = 1;
    // The shell's own state, which a reload of the page does not reset.
    const SHELL = "__tauriShell";
    let shell: { start: string | null } | null = null;
    try {
      shell = JSON.parse(sessionStorage.getItem(SHELL) ?? "null");
    } catch {
      shell = null;
    }
    if (!shell) shell = { start: start.start ?? null };
    const keep = () => {
      try {
        sessionStorage.setItem(SHELL, JSON.stringify(shell));
      } catch {
        /* a page with no storage (about:blank): nothing to keep */
      }
    };
    keep();
    const files = start.files ?? {};
    const scope = new Set<string>();
    /** The bytes of a file in the scope, as the fs plugin would read them. */
    const read = (path: string): Uint8Array => {
      mock.reads.push(path);
      if (!scope.has(path)) throw `forbidden path: ${path}`;
      const b64 = files[path];
      if (b64 === undefined || b64 === null) throw `failed to open file at path: ${path} with error: The system cannot find the file specified. (os error 2)`;
      return Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    };
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
      allowed: [],
      reads: [],
      titles: [],
      titleFails: false,
      request() {
        if (mock.unsaved) mock.emit("sawhorse://close-requested", null);
        else mock.ended = "closed";
      },
      emit(event, payload = null) {
        for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id, payload });
      },
      listeners: (event) => (listeners.get(event) ?? []).length,
      drop(paths) {
        for (const p of paths) {
          scope.add(p);
          mock.allowed.push(p);
        }
        mock.emit("sawhorse://dropped", paths);
      },
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
            case "start_file": {
              // lib.rs: taken once; exactly that path goes into the fs scope, then it is returned.
              const path = shell!.start;
              shell!.start = null;
              keep();
              if (path === null) return null;
              scope.add(path);
              mock.allowed.push(path);
              return path;
            }
            case "window_title": {
              // lib.rs: the text goes through sawhorse_window_title::title onto this window.
              if (mock.titleFails) throw new Error("window_title did not reach the shell");
              mock.titles.push(String(args.title));
              return null;
            }
            case "plugin:fs|stat": {
              const bytes = read(String(args.path));
              const none = { mtime: null, atime: null, birthtime: null, fileAttributes: null, dev: null, ino: null, mode: null, nlink: null, uid: null, gid: null, rdev: null, blksize: null, blocks: null };
              return { isFile: true, isDirectory: false, isSymlink: false, size: bytes.length, readonly: false, ...none };
            }
            case "plugin:fs|read_file":
              return read(String(args.path)).buffer;
            default:
              throw new Error(`${cmd} is not in this stand-in for the desktop shell`);
          }
        })();
      },
    };
  }, { platform, start });
}

/** The stand-in's record, read from the page. */
export async function bridge(page: Page): Promise<Pick<TauriMock, "log" | "said" | "asked" | "unsaved" | "ended" | "allowed" | "reads" | "titles">> {
  return page.evaluate(() => {
    const m = (window as unknown as { __tauriMock: TauriMock }).__tauriMock;
    return { log: [...m.log], said: [...m.said], asked: [...m.asked], unsaved: m.unsaved, ended: m.ended, allowed: [...m.allowed], reads: [...m.reads], titles: [...m.titles] };
  });
}

/** Change how the stand-in behaves. */
export async function setBridge(page: Page, over: Partial<Pick<TauriMock, "answer" | "delay" | "dialog" | "failWords" | "titleFails">>): Promise<void> {
  await page.evaluate((o) => Object.assign((window as unknown as { __tauriMock: TauriMock }).__tauriMock, o), over);
}

/** A close request from the system (the window's X, Alt+F4). */
export async function closeWindow(page: Page): Promise<void> {
  await page.evaluate(() => (window as unknown as { __tauriMock: TauriMock }).__tauriMock.request());
}

/** Files dropped on the window (allowed in the scope, then sawhorse://dropped). */
export async function dropFiles(page: Page, paths: string[]): Promise<void> {
  await page.evaluate((p) => (window as unknown as { __tauriMock: TauriMock }).__tauriMock.drop(p), paths);
}

/**
 * A new start of the desktop app, started with `start` (or with no file): the page loads
 * again and the shell has a start-up file to hand over once more. The page's storage, the
 * autosave with it, is kept, as it is between two runs of the app.
 */
export async function restart(page: Page, start: string | null): Promise<void> {
  await page.evaluate((s) => sessionStorage.setItem("__tauriShell", JSON.stringify({ start: s })), start);
  await page.reload();
}
