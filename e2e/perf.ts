import { test, type Page } from "@playwright/test";

// Lane 16b: frame-gap bounds that hold on any machine (docs/DEVELOPMENT.md, "Responsiveness
// bounds in the end-to-end tests").
//
// A fixed bound in milliseconds measures the machine, not the app: lane 16's "select all at
// 100,000 solids" bound of 6 s held with room to spare on the development machine (0.5 s)
// and failed on GitHub's runners (19 to 31 s), which have two or four shared CPUs and no
// GPU (WebGL there is SwiftShader, the GPU emulated on the CPU). So each spec first measures
// the machine it runs on, in the page, and states its bound in those units:
//
//   cpu   the median time of a fixed pure-JavaScript workload (fill and sort 200,000
//         doubles, then 100,000 string-keyed map entries): the page's main-thread speed,
//         CPU throttling and a busy runner included.
//   frame the median time of one full render of the model as it stands, waited for by
//         reading a pixel back: the GPU's speed (or the emulator's) on this very scene.
//
//   bound = min(CEILING_MS, max(FLOOR_MS, kCpu * cpu + kFrame * frame))
//
// Each check names its kCpu and kFrame: about three times what the work measured on the
// development machine (GPU, and under 6x CPU throttling) and under SwiftShader (1x and 4x
// throttling), in those units. The floor keeps a fast machine's bound from being so tight
// that a garbage collection fails it; the ceiling is a true freeze on any machine: no
// calibration, however slow the runner, lets a 20-second gap pass. PERF_ABSOLUTE_MS (the
// nightly job on a dedicated runner, nightly-perf.yml) replaces every bound with that
// absolute number.

/** No bound is ever below this (a fast machine's garbage collection must not fail it). */
export const FLOOR_MS = 1500;
/** No bound is ever above this: a freeze of this long fails on any machine. */
export const CEILING_MS = 20_000;

export interface Calibration {
  /** Milliseconds for the fixed JavaScript workload (median of five, after one warm-up). */
  cpu: number;
  /** Milliseconds for one full render of the current scene, GPU work included (median of three). */
  frame: number;
}

/** Measure this machine (call it when nothing else is running in the page). */
export async function calibrate(page: Page): Promise<Calibration> {
  const cpu = await page.evaluate(() => {
    const run = () => {
      const t = performance.now();
      const a = new Float64Array(200_000);
      for (let i = 0; i < a.length; i++) a[i] = (Math.sin(i * 12.9898) * 43758.5453) % 1;
      a.sort();
      const m = new Map<string, number>();
      for (let i = 0; i < 100_000; i++) m.set("k" + i, a[i]);
      let x = 0;
      for (const [k, v] of m) x += v * k.length;
      // Use x, so the work cannot be optimised away.
      return performance.now() - t + (Number.isNaN(x) ? 1 : 0);
    };
    run();
    const r = [run(), run(), run(), run(), run()].sort((p, q) => p - q);
    return r[2];
  });
  const frames: number[] = [];
  for (let k = 0; k < 3; k++) {
    frames.push(
      await page.evaluate(() => {
        const w = (window as unknown as { __sawhorse: { frameTime(n: number): number; pixel(x: number, y: number): unknown } }).__sawhorse;
        // Read back the canvas's centre (the model is there): an emulated GPU waits only for
        // the work drawn where the read is, so a corner would return before the frame is done.
        const c = document.querySelector<HTMLCanvasElement>("#cv")!.getBoundingClientRect();
        const [x, y] = [Math.round(c.width / 2), Math.round(c.height / 2)];
        w.pixel(x, y);
        const t = performance.now();
        w.frameTime(1);
        w.pixel(x, y);
        return performance.now() - t;
      }),
    );
  }
  frames.sort((p, q) => p - q);
  return { cpu, frame: frames[1] };
}

/** The bound for one check: `kCpu` workloads and `kFrame` renders of this machine. */
export function frameBound(c: Calibration, kCpu: number, kFrame: number): number {
  const absolute = Number(process.env.PERF_ABSOLUTE_MS);
  if (absolute > 0) return absolute;
  return Math.round(Math.min(CEILING_MS, Math.max(FLOOR_MS, kCpu * c.cpu + kFrame * c.frame)));
}

/** Record a measurement with its bound (in the report, and in the log for CI). */
export function note(what: string, gap: number, bound: number, c: Calibration): void {
  const line = `${what}: longest frame gap ${Math.round(gap)} ms, bound ${bound} ms (cpu ${c.cpu.toFixed(1)} ms, frame ${c.frame.toFixed(1)} ms${process.env.PERF_ABSOLUTE_MS ? `, absolute ${process.env.PERF_ABSOLUTE_MS} ms` : ""})`;
  test.info().annotations.push({ type: "frame gap", description: line });
  console.log(`[perf] ${line}`);
}
