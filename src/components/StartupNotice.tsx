// #115: a file the desktop app was started with waits for the answer to "Restore unsaved
// work?" (src/io/startup.ts). The question was only in the status line, so the empty model
// in front of the person looked like a file that opened empty. While a file waits, the same
// question, with the same buttons and the waiting file's name, is shown over the model area
// too; after Restore, so is the file's Open button. With no file waiting, the offer is in the
// status line only, as before. Not a system dialog: the native one has two answers, and no
// room for the Open button after Restore.
import type { StatusMessage } from "./ValueBar";

/**
 * What the notice over the model shows: while a start-up file `waiting`, the offer, the very
 * message the status line shows when it shows it, else the offer made again by `offer` (another
 * message took the status line, and the file still waits); with no file waiting, the
 * status line's message after Restore that offers the file, while it is there.
 */
export function noticeMessage(waiting: string | null, message: StatusMessage | null, offer: () => StatusMessage | null): StatusMessage | null {
  if (waiting !== null) return message?.offer ? message : offer();
  return message?.startup ? message : null;
}

export function StartupNotice({ message }: { message: StatusMessage }) {
  return (
    <div className={message.error ? "startup-notice err" : "startup-notice"} role="region" aria-label="Unsaved work and the file you opened" data-testid="startup-notice">
      <p>{message.text}</p>
      {message.actions?.length ? (
        <div className="startup-actions">
          {message.actions.map((a) => (
            <button key={a.label} type="button" onClick={a.run}>
              {a.label}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}
