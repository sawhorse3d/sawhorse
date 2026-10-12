import { expect, test, type Page } from "@playwright/test";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

// Lane 12: open a document written by the same WASM kernel the app runs, from a golden's
// op log (plus any ops after it), through the File input (?io=basic).

type Wasm = { initSync(o: { module: Buffer }): void; document_replay(ops: string): { save_shz(u: string, t: Uint8Array): Uint8Array; free(): void } };

export async function goldenShz(golden: string, more: unknown[] = [], filter: (op: { op: string }) => boolean = () => true): Promise<Buffer> {
  const pkg = path.join("src", "wasm", "pkg");
  test.skip(!existsSync(path.join(pkg, "sawhorse_wasm.js")), "kernel WASM not built");
  const mod = (await import(pathToFileURL(path.resolve(pkg, "sawhorse_wasm.js")).href)) as Wasm;
  mod.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
  const g = JSON.parse(readFileSync(path.join("crates", "kernel", "tests", "golden", `${golden}.json`), "utf8")) as { ops: { op: string }[] };
  const doc = mod.document_replay(JSON.stringify([...g.ops.filter(filter), ...more]));
  const shz = doc.save_shz("frac-in", new Uint8Array());
  doc.free();
  return Buffer.from(shz);
}

/** Open the app (light, no autosave, basic file input) and the document. */
export async function openShz(page: Page, shz: Buffer, objects: number, name = "model.shz") {
  await page.goto("/?io=basic&theme=light&autosave=0");
  await expect(page.locator("main.vp")).toHaveAttribute("data-kernel", "ready", { timeout: 15_000 });
  await page.getByTestId("file-input").setInputFiles({ name, mimeType: "application/vnd.sawhorse.shz", buffer: shz });
  await expect(page.locator("main.vp")).toHaveAttribute("data-objects", String(objects), { timeout: 15_000 });
}
