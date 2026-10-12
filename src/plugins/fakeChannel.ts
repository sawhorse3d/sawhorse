// A stand-in for a plugin's worker in unit tests: records what the host sends, answers
// init and pings like the sandbox would (unless told to go quiet), and lets a test speak as
// the plugin.
import type { HostMessage, PluginMessage } from "../../packages/plugin-api/src/rpc.ts";
import type { Channel } from "./host";

export interface FakeChannel extends Channel {
  posted: HostMessage[];
  terminated: boolean;
  /** Answer pings (false: the plugin is stuck in a loop). */
  alive: boolean;
  /** Say ready on init (false: say nothing; a string: fail with it). */
  onInit: true | false | string;
  fromPlugin(msg: PluginMessage | Record<string, unknown>): void;
  failWorker(message: string): void;
  /** The last request answer for request `id`. */
  answer(id: number): HostMessage | undefined;
}

export function fakeChannel(): FakeChannel {
  let onMsg: (raw: unknown) => void = () => undefined;
  let onErr: (m: string) => void = () => undefined;
  const ch: FakeChannel = {
    posted: [],
    terminated: false,
    alive: true,
    onInit: true,
    post(msg) {
      if (ch.terminated) return;
      ch.posted.push(msg);
      if (msg.t === "init") {
        if (ch.onInit === true) queueMicrotask(() => onMsg({ t: "ready" }));
        else if (typeof ch.onInit === "string") {
          const error = ch.onInit;
          queueMicrotask(() => onMsg({ t: "fail", error }));
        }
      }
      if (msg.t === "ping" && ch.alive) queueMicrotask(() => onMsg({ t: "pong", n: msg.n }));
    },
    onMessage(cb) {
      onMsg = cb;
    },
    onError(cb) {
      onErr = cb;
    },
    terminate() {
      ch.terminated = true;
    },
    fromPlugin(msg) {
      onMsg(msg);
    },
    failWorker(message) {
      onErr(message);
    },
    answer(id) {
      return [...ch.posted].reverse().find((m) => m.t === "res" && m.id === id);
    },
  };
  return ch;
}
