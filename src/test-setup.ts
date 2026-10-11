// jsdom lacks a few browser APIs the shell touches.
class NoopResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
const g = globalThis as unknown as Record<string, unknown>;
if (!("ResizeObserver" in g)) g.ResizeObserver = NoopResizeObserver;
if (typeof window !== "undefined" && !window.matchMedia) {
  window.matchMedia = ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}
// jsdom's canvas has no WebGL; the viewport shows its "WebGL2 unavailable" HUD chip instead.
if (typeof HTMLCanvasElement !== "undefined") {
  HTMLCanvasElement.prototype.getContext = (() => null) as unknown as typeof HTMLCanvasElement.prototype.getContext;
}

// The render service is never reached from unit tests: the default credits client is offline
// (components that need a live one get a fake through useCredits(client)).
import { setDefaultCreditsClient } from "./credits/useCredits";
import { RenderServiceClient } from "./credits/client";
setDefaultCreditsClient(
  () =>
    new RenderServiceClient(
      "http://render.test.invalid",
      async () => ({ installId: "aaaaaaaaaaaaaaaaaaaa", publicKey: new Uint8Array(32), storage: "session-only", sign: async () => new Uint8Array(64) }),
      async () => {
        throw new TypeError("offline in unit tests");
      },
    ),
);
