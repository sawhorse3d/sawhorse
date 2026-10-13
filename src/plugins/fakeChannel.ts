// A stand-in for a plugin's worker in unit tests: records what the host sends (each frame
// read back with the plugin side's own decoder), answers init and pings like the sandbox
// would (unless told to go quiet), and lets a test speak as the plugin. What a test sends as
// the plugin goes through the same serializer the sandbox uses (lane 16); `fromPluginRaw`
// sends anything at all, the way a broken or hostile sandbox could.
import { decodeHostFrame, encodePluginMessage, type HostMessage, type PluginMessage } from "../../packages/plugin-api/src/rpc.ts";
import type { Channel } from "./host";

export interface FakeChannel extends Channel {
  posted: HostMessage[];
  /** The raw frames the host posted, in order (sizes, budgets). */
  frames: ArrayBuffer[];
  terminated: boolean;
  /** Answer pings (false: the plugin is stuck in a loop). */
  alive: boolean;
  /** Say ready on init (false: say nothing; a string: fail with it). */
  onInit: true | false | string;
  /** Speak as the plugin: serialized like the sandbox does (a message the serializer refuses is sent raw). */
  fromPlugin(msg: PluginMessage | Record<string, unknown>): void;
  /** Send anything as the plugin (not a frame, a forged frame, a huge buffer ...). */
  fromPluginRaw(raw: unknown): void;
  failWorker(message: string): void;
  /** The last request answer for request `id`. */
  answer(id: number): HostMessage | undefined;
}

export function fakeChannel(): FakeChannel {
  let onMsg: (raw: unknown) => void = () => undefined;
  let onErr: (m: string) => void = () => undefined;
  const say = (m: PluginMessage) => onMsg(encodePluginMessage(m));
  const ch: FakeChannel = {
    posted: [],
    frames: [],
    terminated: false,
    alive: true,
    onInit: true,
    post(frame) {
      if (ch.terminated) return;
      ch.frames.push(frame);
      const msg = decodeHostFrame(frame);
      ch.posted.push(msg);
      if (msg.t === "init") {
        if (ch.onInit === true) queueMicrotask(() => say({ t: "ready" }));
        else if (typeof ch.onInit === "string") {
          const error = ch.onInit;
          queueMicrotask(() => say({ t: "fail", error }));
        }
      }
      if (msg.t === "ping" && ch.alive) queueMicrotask(() => say({ t: "pong", n: msg.n }));
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
      let frame: ArrayBuffer | null = null;
      try {
        frame = encodePluginMessage(msg as PluginMessage);
      } catch {
        frame = null;
      }
      onMsg(frame ?? msg);
    },
    fromPluginRaw(raw) {
      onMsg(raw);
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
