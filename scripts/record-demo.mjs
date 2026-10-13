#!/usr/bin/env node
// The site's silent demo video (lane 15): Playwright drives the built app at 1280 x 800 and
// records it; ffmpeg turns the recording into a small looping WebM and a poster.
//
//   npm run build && npx vite preview --port 4215 --strictPort      (in another terminal)
//   node scripts/record-demo.mjs --url http://localhost:4215 [--out site/media]
//
// What it shows (about 50 s): a bookshelf .shz written by the WASM kernel from the cut-list
// golden opens, the view turns round it, the Cut list pane lists its parts, Nesting lays
// them out on boards and a sheet, one cutting diagram is looked at, and the PDF booklet is
// saved. Writes <out>/demo.webm (VP9, no audio, under 4 MB) and <out>/demo-poster.png; with
// no ffmpeg on the PATH it writes <out>/demo.webp instead (12 frames, animated, under 3 MB).
// scripts/check-site.mjs holds the site to those caps.
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "@playwright/test";

const root = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const argv = process.argv.slice(2);
const opt = (name, dflt) => {
  const i = argv.indexOf(`--${name}`);
  return i >= 0 && argv[i + 1] ? argv[i + 1] : dflt;
};
const url = opt("url", "http://localhost:4215").replace(/\/+$/, "");
const out = path.resolve(root, opt("out", "site/media"));
mkdirSync(out, { recursive: true });
const W = 1280;
const H = 800;

const pkg = path.join(root, "src", "wasm", "pkg");
const mod = await import(pathToFileURL(path.join(pkg, "sawhorse_wasm.js")).href);
mod.initSync({ module: readFileSync(path.join(pkg, "sawhorse_wasm_bg.wasm")) });
const golden = JSON.parse(readFileSync(path.join(root, "crates", "kernel", "tests", "golden", "cutlist.json"), "utf8"));
// The golden's bookshelf as a woodworker would draw it: without its test-only edits (the top's
// grain turned across, a hidden shelf, a mirrored copy of a side).
const ops = golden.ops.filter((o, i) => i < 23 || o.op === "set_stock");
const doc = mod.document_replay(JSON.stringify(ops));
const shz = Buffer.from(doc.save_shz("frac-in", new Uint8Array()));
doc.free();

const tmp = mkdtempSync(path.join(tmpdir(), "sawhorse-demo-"));
const browser = await chromium.launch({ channel: process.env.PW_CHANNEL ?? "msedge" });
const frames = [];
try {
  const context = await browser.newContext({ viewport: { width: W, height: H }, deviceScaleFactor: 1, acceptDownloads: true });
  const page = await context.newPage();
  // The browser's own screencast (no Playwright video download needed): a JPEG per painted
  // frame with its time, joined by ffmpeg below.
  const cast = [];
  const cdp = await context.newCDPSession(page);
  cdp.on("Page.screencastFrame", (f) => {
    const file = path.join(tmp, `cast-${String(cast.length).padStart(5, "0")}.jpg`);
    writeFileSync(file, Buffer.from(f.data, "base64"));
    cast.push({ file, t: f.metadata.timestamp });
    void cdp.send("Page.screencastFrameAck", { sessionId: f.sessionId }).catch(() => {});
  });
  await cdp.send("Page.startScreencast", { format: "jpeg", quality: 85, maxWidth: W, maxHeight: H, everyNthFrame: 1 });
  page.setDefaultTimeout(20_000);
  const still = async () => {
    // A frame for the animated WebP fallback.
    const f = path.join(tmp, `frame-${String(frames.length).padStart(2, "0")}.png`);
    await page.screenshot({ path: f });
    frames.push(f);
  };
  const pause = (ms) => page.waitForTimeout(ms);
  await page.goto(`${url}/?theme=light&autosave=0&io=basic`);
  await page.waitForSelector('main.vp[data-kernel="ready"]', { timeout: 30_000 });
  await pause(1200);
  await page.getByTestId("file-input").setInputFiles({ name: "bookshelf.shz", mimeType: "application/vnd.sawhorse.shz", buffer: shz });
  await page.waitForFunction(() => Number(document.querySelector("main.vp")?.getAttribute("data-objects")) >= 9);
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await page.keyboard.press("Shift+Z");
  await page.locator(".import-report button").first().click().catch(() => {});
  await pause(2500);
  await still();
  // Turn round the bookshelf: a middle-button drag across the view, slowly.
  const cv = await page.locator("#cv").boundingBox();
  const cx = cv.x + cv.width / 2;
  const cy = cv.y + cv.height / 2;
  await page.mouse.move(cx, cy);
  await page.mouse.down({ button: "middle" });
  for (let k = 0; k <= 60; k++) {
    await page.mouse.move(cx - 220 * Math.sin((k / 60) * Math.PI), cy - 40 * Math.sin((k / 60) * Math.PI * 2), { steps: 2 });
    await pause(70);
    if (k === 30) await still();
  }
  await page.mouse.up({ button: "middle" });
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await page.keyboard.press("Shift+Z");
  await pause(1500);
  await still();
  // The cut list.
  await page.getByRole("tab", { name: "Cut list" }).click();
  await page.waitForSelector(".cutlist table");
  await pause(3500);
  await still();
  // Hover down the rows: each lights its pieces.
  for (const name of ["Side", "Top", "Shelf", "Back panel"]) {
    await page.locator(".cutlist tbody tr", { hasText: name }).first().hover();
    await pause(900);
  }
  await page.locator(".cutlist tbody tr", { hasText: "Shelf" }).first().click();
  await pause(2000);
  await still();
  // Nesting: boards and a sheet, the diagrams.
  await page.getByRole("button", { name: /Nesting ›/ }).click();
  await page.waitForSelector('[data-testid="nesting"] figure');
  await pause(3000);
  await still();
  const figs = page.locator('[data-testid="nesting"] figure');
  const n = await figs.count();
  for (let i = 0; i < Math.min(n, 4); i++) {
    await figs.nth(i).scrollIntoViewIfNeeded();
    await figs.nth(i).hover();
    await pause(1800);
    if (i === 1) await still();
  }
  // A cutting diagram close up: hover its parts.
  const parts = figs.first().locator("[data-tag], rect");
  const pn = Math.min(await parts.count(), 6);
  for (let i = 0; i < pn; i++) {
    await parts.nth(i).hover().catch(() => {});
    await pause(500);
  }
  await still();
  // The PDF booklet.
  const pdf = page.getByRole("button", { name: "PDF", exact: true }).last();
  await pdf.scrollIntoViewIfNeeded();
  await pdf.hover();
  await pause(800);
  const dl = page.waitForEvent("download", { timeout: 15_000 }).catch(() => null);
  await pdf.click();
  await dl;
  await pause(2500);
  await still();
  // Back to the whole model.
  await page.getByRole("tab", { name: "Outliner" }).click();
  await page.keyboard.press("Escape");
  await pause(3000);
  await still();
  await cdp.send("Page.stopScreencast");
  await context.close();
  if (cast.length < 10) throw new Error(`only ${cast.length} frames came from the screencast`);
  // ffmpeg's concat list: each frame held until the next one came (the screen only sends a
  // frame when something changed), the first second (loading) dropped.
  const t0 = cast[0].t + 1;
  const kept = cast.filter((c) => c.t >= t0);
  const lines = [];
  for (let i = 0; i < kept.length; i++) {
    const next = i + 1 < kept.length ? kept[i + 1].t : kept[i].t + 1.5;
    lines.push(`file '${kept[i].file.replace(/\\/g, "/")}'`, `duration ${Math.max(0.01, next - kept[i].t).toFixed(3)}`);
  }
  lines.push(`file '${kept[kept.length - 1].file.replace(/\\/g, "/")}'`);
  const list = path.join(tmp, "frames.txt");
  writeFileSync(list, lines.join("\n"));
  const total = kept[kept.length - 1].t + 1.5 - kept[0].t;
  console.log(`[demo] ${kept.length} screencast frames over ${total.toFixed(1)} s`);
  // The site wants 45 to 60 s: a longer run plays a little faster, evenly.
  const speed = Math.max(1, total / 56);

  const ff = spawnSync("ffmpeg", ["-version"], { encoding: "utf8" });
  if (ff.status === 0) {
    const webm = path.join(out, "demo.webm");
    const poster = path.join(out, "demo-poster.png");
    // VP9, constant quality, no audio, 15 fps, the first second (the page loading) dropped.
    const args = ["-y", "-f", "concat", "-safe", "0", "-i", list, "-an", "-vf", `setpts=PTS/${speed.toFixed(4)},fps=15,scale=${W}:${H}:flags=lanczos,format=yuv420p`, "-c:v", "libvpx-vp9", "-b:v", "0", "-crf", "42", "-row-mt", "1", "-deadline", "good", "-cpu-used", "2", webm];
    const r = spawnSync("ffmpeg", args, { encoding: "utf8" });
    if (r.status !== 0) throw new Error(`ffmpeg: ${r.stderr.slice(-2000)}`);
    const p = spawnSync("ffmpeg", ["-y", "-i", frames[0] ?? kept[0].file, "-frames:v", "1", "-vf", "scale=960:-2:flags=lanczos,split[a][b];[a]palettegen=max_colors=96[p];[b][p]paletteuse=dither=none", poster], { encoding: "utf8" });
    if (p.status !== 0) throw new Error(`ffmpeg poster: ${p.stderr.slice(-2000)}`);
    const probe = spawnSync("ffprobe", ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", webm], { encoding: "utf8" });
    console.log(`[demo] ${path.relative(root, webm)}: ${Math.round(statSync(webm).size / 1024)} KB, ${Number(probe.stdout).toFixed(1)} s; poster ${Math.round(statSync(poster).size / 1024)} KB`);
  } else {
    // No ffmpeg: twelve stills as an animated WebP (the page's fallback).
    const webp = path.join(out, "demo.webp");
    const page2 = await (await browser.newContext()).newPage();
    const data = frames.slice(0, 12).map((f) => readFileSync(f).toString("base64"));
    const bytes = await page2.evaluate(async (pngs) => {
      // Encode each frame as WebP and join them into an animated WebP (ANIM/ANMF chunks).
      const enc = async (b64) => {
        const img = new Image();
        img.src = `data:image/png;base64,${b64}`;
        await img.decode();
        const c = new OffscreenCanvas(img.width, img.height);
        c.getContext("2d").drawImage(img, 0, 0);
        return new Uint8Array(await (await c.convertToBlob({ type: "image/webp", quality: 0.6 })).arrayBuffer());
      };
      const out = [];
      for (const p of pngs) out.push(Array.from(await enc(p)));
      return out;
    }, data);
    writeFileSync(webp, animatedWebp(bytes.map((b) => Uint8Array.from(b)), W, H, 1500));
    console.log(`[demo] no ffmpeg: ${path.relative(root, webp)} (${Math.round(statSync(webp).size / 1024)} KB, ${Math.min(12, frames.length)} frames)`);
  }
} finally {
  await browser.close();
  rmSync(tmp, { recursive: true, force: true });
}

/** An animated WebP from still WebP frames (VP8/VP8L bitstreams), each shown `ms`. */
function animatedWebp(stills, w, h, ms) {
  const le = (n, bytes) => Array.from({ length: bytes }, (_, i) => (n >> (8 * i)) & 255);
  const chunk = (tag, data) => {
    const pad = data.length % 2 ? [0] : [];
    return [...Buffer.from(tag), ...le(data.length, 4), ...data, ...pad];
  };
  const vp8x = chunk("VP8X", [0x02, 0, 0, 0, ...le(w - 1, 3), ...le(h - 1, 3)]);
  const anim = chunk("ANIM", [255, 255, 255, 255, 0, 0]);
  const frames = stills.map((s) => {
    // The still's own image chunk (after RIFF....WEBP).
    const inner = Array.from(s.slice(12));
    return chunk("ANMF", [...le(0, 3), ...le(0, 3), ...le(w - 1, 3), ...le(h - 1, 3), ...le(ms, 3), 0x02, ...inner]);
  });
  const body = [...Buffer.from("WEBP"), ...vp8x, ...anim, ...frames.flat()];
  return Uint8Array.from([...Buffer.from("RIFF"), ...le(body.length, 4), ...body]);
}

