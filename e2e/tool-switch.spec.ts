import { expect, test, type Page } from "@playwright/test";
import { ready } from "./helpers";

// A tool key that waits for an edit, then a value typed for the new tool (alpha.3 fix pass).
//
// "r", "12,12", Enter draws a rectangle; "p" right after it has to wait for that edit (Push/Pull
// must see the rectangle), so the switch happens later, outside the key's own event. If the
// next keys ("6", Enter) arrive before React has rendered that switch, the render they cause
// must not hand the keyboard back to the old tool: the 6 belongs to Push/Pull. On a loaded CI
// runner this lost the 6 to Rect ("Rect takes two sizes separated by a comma") in move.spec
// and containment.spec. Here the timing is forced: the kernel's reply is held, then released,
// the switch runs, and "6" and Enter are dispatched in the same task, before React's render
// of the switch could run.

declare global {
  interface Window {
    __kernelHold: { on: boolean; held: (() => void)[] };
  }
}

async function holdKernelReplies(page: Page) {
  await page.addInitScript(() => {
    const Real = window.Worker;
    window.__kernelHold = { on: false, held: [] };
    class HeldWorker extends Real {
      constructor(url: string | URL, opts?: WorkerOptions) {
        super(url, opts);
        if (opts?.name !== "sawhorse-kernel") return;
        let handler: ((e: MessageEvent) => void) | null = null;
        // The client sets onmessage; replies go through here so the test can hold them.
        Object.defineProperty(this, "onmessage", { configurable: true, get: () => handler, set: (f) => (handler = f) });
        this.addEventListener("message", (e) => {
          const deliver = () => handler?.call(this, e as MessageEvent);
          if (window.__kernelHold.on) window.__kernelHold.held.push(deliver);
          else deliver();
        });
      }
    }
    window.Worker = HeldWorker as typeof Worker;
  });
}

test("a value typed right after a tool key that waited for an edit goes to the new tool", async ({ page }) => {
  await holdKernelReplies(page);
  await ready(page);
  await page.keyboard.press("r");
  await page.keyboard.type("12,12");
  await page.evaluate(() => (window.__kernelHold.on = true));
  await page.keyboard.press("Enter");
  // The rectangle's edit is with the kernel, its reply held: "p" has to wait for it.
  await expect.poll(() => page.evaluate(() => window.__kernelHold.held.length)).toBeGreaterThan(0);
  await page.keyboard.press("p");
  await page.evaluate(async () => {
    const tick = async (n: number) => {
      for (let i = 0; i < n; i++) await Promise.resolve();
    };
    const key = (target: EventTarget, k: string) => target.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));
    // One task from here on: microtasks run, React's scheduled render of the switch does not.
    window.__kernelHold.on = false;
    for (const deliver of window.__kernelHold.held.splice(0)) deliver();
    await tick(500); // the edit lands, then the waiting "p" switches to Push/Pull
    key(document.activeElement ?? document.body, "6");
    await tick(50); // the 6's own (synchronous) render
    key(document.getElementById("vcb")!, "Enter");
  });
  const hint = page.locator("#hint");
  await expect(hint).toContainText("Extruded");
  await expect(hint).toContainText("6");
  await expect(page.locator(".rail button[data-tool=\"pushpull\"]")).toHaveAttribute("aria-pressed", "true");
  const vol = await page.evaluate(() => (window as unknown as { __sawhorse: { solids(): { volume: number }[] } }).__sawhorse.solids().map((s) => s.volume));
  expect(vol).toHaveLength(1);
  expect(vol[0] / 0.0254 ** 3).toBeCloseTo(12 * 12 * 6, 3);
});
