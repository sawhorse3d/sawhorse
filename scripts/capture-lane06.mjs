#!/usr/bin/env node
// Lane 6 "verify by looking": drive the running app (npm run dev) through the file flows with
// real clicks, keys and file pickers, and screenshot every state:
//
//   01 a corpus .skp imported (a house: groups, tags, a window wall), with its import report
//   02 File > Export open (every writer)
//   03 the STL we exported, re-imported into a new model
//   04 the 3MF we exported, re-imported beside it (named parts, colours)
//   05 the model saved as .shz, a new model, the .shz opened again
//   06 an open tray (.skp): kept as a visible amber mesh, with the reason
//   07 Export refused for that mesh, by name
//
//   node scripts/capture-lane06.mjs [--url http://localhost:5176] [--out ../../../_captures]
//                                   [--files ../../../_scratch/2026-10-11-lane06]
//
// Needs `npm run corpus:skp` first. Uses the installed Edge (PW_CHANNEL=msedge by default).
// The exported files are kept in --files so they can be opened in other programs.
import { chromium } from "@playwright/test";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { strFromU8, unzipSync } from "fflate";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith("--") ? [...acc, [a.slice(2), all[i + 1]]] : acc), []),
);
const url = args.url ?? "http://localhost:5176";
const outDir = path.resolve(args.out ?? "../_captures");
const filesDir = path.resolve(args.files ?? "../_scratch/2026-10-11-lane06");
const stamp = "2026-10-11-lane06";
mkdirSync(outDir, { recursive: true });
mkdirSync(filesDir, { recursive: true });

const browser = await chromium.launch({ channel: process.env.PW_CHANNEL ?? "msedge" });
let failed = false;
const check = (ok, what) => {
  console.log(`${ok ? "ok  " : "FAIL"} ${what}`);
  if (!ok) failed = true;
};
try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1, acceptDownloads: true });
  const page = await context.newPage();
  page.on("dialog", (d) => void d.accept());
  await page.goto(`${url}/?io=basic&theme=light`);
  await page.waitForSelector('main.vp[data-kernel="ready"]', { timeout: 30_000 });
  const vp = page.locator("main.vp");
  const report = page.getByTestId("import-report");
  const shot = async (n, name) => {
    const file = path.join(outDir, `${stamp}-${n}-${name}.png`);
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    await page.screenshot({ path: file });
    console.log(`     -> ${file}`);
  };
  const state = async () => ({
    objects: await vp.getAttribute("data-objects"),
    volume: Number(await vp.getAttribute("data-volume")),
    watertight: await vp.getAttribute("data-watertight"),
    hud: (await page.locator(".hud").innerText()).replace(/\s+/g, " "),
  });
  const pick = (file) => page.getByTestId("file-input").setInputFiles(file);
  const menu = async (top, ...path) => {
    await page.getByRole("button", { name: top, exact: true }).click();
    for (const [i, label] of path.entries()) {
      const item = page.getByRole("menuitem", { name: label, exact: true });
      if (i < path.length - 1) await item.hover();
      else await item.click();
    }
  };
  const download = async (action, keep) => {
    const [d] = await Promise.all([page.waitForEvent("download"), action()]);
    const to = path.join(filesDir, keep ?? d.suggestedFilename());
    await d.saveAs(to);
    return { path: to, bytes: readFileSync(to) };
  };
  const frame = async () => {
    await page.getByRole("button", { name: "Iso", exact: true }).click();
    await page.keyboard.press("Shift+Z");
  };

  // 01: a corpus house through openskp.
  await pick(path.resolve("tests/corpus/skp/004-house.skp"));
  await report.waitFor();
  await frame();
  const s1 = await state();
  console.log("01", s1, "|", (await report.innerText()).replace(/\s+/g, " "));
  check(s1.objects === "5" && s1.watertight === "true", "house.skp: 5 watertight solids");
  await shot("01", "skp-import");

  // 02: the Export submenu.
  await report.getByRole("button").click();
  await page.getByRole("button", { name: "File", exact: true }).click();
  await page.getByRole("menuitem", { name: "Export", exact: true }).hover();
  await shot("02", "export-menu");
  await page.keyboard.press("Escape");

  // Export STL and 3MF.
  const stl = await download(() => menu("File", "Export", "STL…"), "house.stl");
  const tris = stl.bytes.readUInt32LE(80);
  check(stl.bytes.length === 84 + tris * 50 && tris > 0, `STL: ${tris} triangles, ${stl.bytes.length} bytes, header "${stl.bytes.subarray(0, 35).toString()}"`);
  const tmf = await download(() => menu("File", "Export", "3MF…"), "house.3mf");
  const parts = unzipSync(new Uint8Array(tmf.bytes));
  const xml = strFromU8(parts["3D/3dmodel.model"]);
  check(Object.keys(parts).sort().join(",") === "3D/3dmodel.model,[Content_Types].xml,_rels/.rels", `3MF: entries ${Object.keys(parts).sort().join(", ")}`);
  check((xml.match(/<object /g) ?? []).length === 5 && xml.includes('unit="millimeter"'), `3MF: ${(xml.match(/<object /g) ?? []).length} objects, names ${[...xml.matchAll(/name="([^"]+)"/g)].map((m) => m[1]).slice(0, 8).join(" / ")}`);
  await download(() => menu("File", "Export", "glTF (GLB)…"), "house.glb");
  await download(() => menu("File", "Export", "DXF (3D)…"), "house.dxf");
  await download(() => menu("File", "Export", "SVG drawing of this view…"), "house-iso.svg");

  // 03: the STL back into a new model.
  await menu("File", "New");
  await pick(stl.path);
  await report.waitFor();
  await frame();
  const s3 = await state();
  console.log("03", s3, "|", (await report.innerText()).replace(/\s+/g, " "));
  check(s3.objects === "5" && Math.abs(s3.volume - s1.volume) < 1e-6 * s1.volume, `STL re-import: ${s3.objects} solids, volume ${s3.volume.toFixed(6)} m3 (was ${s1.volume.toFixed(6)})`);
  await shot("03", "stl-reimport");

  // 04: the 3MF beside it.
  await report.getByRole("button").click();
  await pick(tmf.path);
  await report.waitFor();
  const s4 = await state();
  console.log("04", s4, "|", (await report.innerText()).replace(/\s+/g, " "));
  check(s4.objects === "10", "3MF re-import: 5 more solids");
  await shot("04", "3mf-reimport");

  // 05: save as .shz, new, open again.
  await report.getByRole("button").click();
  const before = await vp.getAttribute("data-hash");
  const shz = await download(() => page.keyboard.press("Control+s"), "house.shz");
  const inside = unzipSync(new Uint8Array(shz.bytes));
  console.log("     .shz entries:", Object.keys(inside).join(", "), `(${shz.bytes.length} bytes)`);
  await menu("File", "New");
  await pick(shz.path);
  await page.waitForFunction(() => document.title.startsWith("house - "));
  await frame();
  check((await vp.getAttribute("data-hash")) === before, `.shz reopened with the same model hash ${before}`);
  writeFileSync(path.join(filesDir, "house-thumbnail.png"), inside["thumbnail.png"]);
  await shot("05", "shz-reopened");

  // 06: an open tray stays a visible mesh, with its reason.
  await menu("File", "New");
  await pick(path.resolve("tests/corpus/skp/009-open.skp"));
  await report.waitFor();
  await frame();
  const s6 = await state();
  console.log("06", s6, "|", (await report.innerText()).replace(/\s+/g, " "));
  check(s6.objects === "0" && s6.hud.includes("1 mesh not solid"), "open tray: a non-solid mesh, shown");
  await shot("06", "open-mesh");

  // 07: STL refuses it by name.
  await report.getByRole("button").click();
  await menu("File", "Export", "STL…");
  await page.waitForFunction(() => document.querySelector(".vcb .hint")?.textContent?.includes("Export refused"), null, { timeout: 10_000 }).catch(() => {});
  const msg = await page.locator(".vcb .hint").innerText();
  console.log("07", msg);
  check(msg.includes("is not a solid"), "STL export refused by name");
  await shot("07", "export-refused");
} catch (e) {
  console.error(e);
  failed = true;
} finally {
  await browser.close();
}
process.exitCode = failed ? 1 : 0;
