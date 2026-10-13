#!/usr/bin/env node
// Check the assembled site (scripts/build-site.mjs) before it is deployed:
//
//   - every page's local links, images and anchors resolve to a file or an id;
//   - no scripts at all on the site pages, and the only outside resource is the Google
//     Fonts stylesheet (no analytics, no cookies, no trackers);
//   - the words that must be there are there (the pitch, the licence, the trademark
//     notice, the unsigned-builds note, the maintainer's handle);
//   - screenshots under 400 KB; CNAME says sawhorse3d.com; .nojekyll present;
//   - the demo video (lane 15): a WebM under 4 MB with a poster (under 400 KB), or an
//     animated WebP under 3 MB, above the fold on the home page; the comparison table with
//     its date and the trademark footnote;
//   - the demo in app/ is built for <base>app/, with the kernel WASM and its worker, and
//     carries its content security policy (the plugin sandbox's second lock);
//   - no machine paths or personal names anywhere in the output;
//   - with --online, every download link answers (follows GitHub's redirect to the file).
//
//   node scripts/check-site.mjs [--dir dist-site] [--base /] [--online]
import { readdirSync, readFileSync, statSync, existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const argv = process.argv.slice(2);
const opt = (name, dflt) => {
  const i = argv.indexOf(`--${name}`);
  return i >= 0 && argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[i + 1] : dflt;
};
const dir = path.resolve(root, opt("dir", "dist-site"));
let base = opt("base", "/");
if (!base.startsWith("/")) base = `/${base}`;
if (!base.endsWith("/")) base = `${base}/`;
const online = argv.includes("--online");

const problems = [];
const bad = (msg) => problems.push(msg);
const MAX_IMG = 400 * 1024;
const MAX_VIDEO = 4 * 1024 * 1024;
const MAX_ANIMATION = 3 * 1024 * 1024;

function walk(d) {
  return readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(path.join(d, e.name)) : [path.join(d, e.name)]));
}
const files = walk(dir);
const rel = (f) => path.relative(dir, f).replace(/\\/g, "/");

// ---- required files -------------------------------------------------------------------
for (const f of ["index.html", "style.css", "favicon.svg", ".nojekyll", "CNAME", "app/index.html", "guide/index.html", "changelog/index.html"]) {
  if (!existsSync(path.join(dir, f))) bad(`missing ${f}`);
}
if (existsSync(path.join(dir, "CNAME")) && readFileSync(path.join(dir, "CNAME"), "utf8").trim() !== "sawhorse3d.com") bad("CNAME is not sawhorse3d.com");

// ---- site pages (everything but the app bundle) ---------------------------------------
const pages = files.filter((f) => f.endsWith(".html") && !rel(f).startsWith("app/"));
const ids = new Map(pages.map((f) => [f, new Set([...readFileSync(f, "utf8").matchAll(/\sid="([^"]+)"/g)].map((m) => m[1]))]));
const downloads = new Set();

function resolveLocal(from, url) {
  const clean = url.split("#")[0].split("?")[0];
  let target = clean === "" ? from : path.resolve(path.dirname(from), decodeURIComponent(clean));
  if (clean.endsWith("/") || (existsSync(target) && statSync(target).isDirectory())) target = path.join(target, "index.html");
  return target;
}

for (const f of pages) {
  const html = readFileSync(f, "utf8");
  const name = rel(f);
  if (/<script\b/i.test(html)) bad(`${name}: has a <script> (the site pages carry no scripts)`);
  if (/\bon[a-z]+\s*=\s*"/i.test(html)) bad(`${name}: inline event handler`);
  if (/(google-analytics|googletagmanager|gtag\(|plausible|matomo|segment\.|hotjar|document\.cookie|set-cookie)/i.test(html)) bad(`${name}: tracking or cookie code`);
  for (const m of html.matchAll(/<link\b[^>]*>/gi)) {
    const href = /href="([^"]+)"/.exec(m[0])?.[1] ?? "";
    if (/^https?:/.test(href) && !/rel="(preconnect|dns-prefetch)"/.test(m[0]) && !href.startsWith("https://fonts.googleapis.com/")) bad(`${name}: outside stylesheet or link ${href}`);
  }
  for (const m of html.matchAll(/\s(src|srcset|poster)="([^"]+)"/gi)) {
    const url = m[2].split(/\s/)[0];
    if (/^(https?:)?\/\//.test(url)) bad(`${name}: loads an outside resource ${url}`);
    else if (!url.startsWith("data:") && !existsSync(resolveLocal(f, url))) bad(`${name}: ${m[1]}="${url}" does not exist`);
  }
  for (const m of html.matchAll(/\shref="([^"]+)"/gi)) {
    const url = m[1].replace(/&amp;/g, "&");
    if (/^(mailto:|https?:)/.test(url)) {
      if (url.includes("/releases/download/")) downloads.add(url);
      continue;
    }
    const target = resolveLocal(f, url);
    if (!existsSync(target)) {
      bad(`${name}: link ${url} does not exist`);
      continue;
    }
    const hash = url.includes("#") ? url.split("#")[1] : "";
    if (hash && target.endsWith(".html") && !(ids.get(target) ?? new Set()).has(hash)) bad(`${name}: anchor ${url} has no matching id`);
  }
  if (!/MIT OR Apache-2\.0/.test(html)) bad(`${name}: no licence line`);
  if (!/Not affiliated with Trimble\. SketchUp is a trademark of Trimble Inc\./.test(html)) bad(`${name}: no trademark notice`);
}

const home = existsSync(path.join(dir, "index.html")) ? readFileSync(path.join(dir, "index.html"), "utf8") : "";
for (const must of [
  "Free, local-first, open-source SketchUp-style modeling for woodworkers and makers",
  "Try it in your browser",
  'href="app/"',
  "unsigned",
  "maintained by @boardfoot",
  'href="guide/"',
  'href="changelog/"',
  "https://github.com/sawhorse3d/sawhorse",
]) {
  if (!home.includes(must)) bad(`index.html: missing "${must}"`);
}
for (const os of ["Windows", "macOS (Apple Silicon)", "Linux"]) if (!home.includes(`<h3>${os}</h3>`)) bad(`index.html: no ${os} download`);
// Lane 15: the comparison, dated, with the trademark footnote; the other free options.
for (const must of ["How it compares", "as of 12 October 2026", "SketchUp is a trademark of Trimble Inc.", "OpenCutList is the work and the name of its authors", "Other free options", "Not yet"]) {
  if (!home.includes(must)) bad(`index.html: missing "${must}"`);
}
// Lane 15: the demo, above the fold (in the hero), silent and looping.
const hero = /<section class="hero">([\s\S]*?)<\/section>/.exec(home)?.[1] ?? "";
const video = /<video\b[^>]*>/.exec(hero)?.[0] ?? "";
if (video) {
  for (const attr of ["autoplay", "muted", "loop", "playsinline", "poster="]) if (!video.includes(attr)) bad(`index.html: the demo video lacks ${attr}`);
  if (!/<source src="[^"]+\.webm" type="video\/webm">/.test(hero)) bad("index.html: the demo video has no WebM source");
} else if (!/<img[^>]+src="[^"]*demo\.webp"/.test(hero)) bad("index.html: no demo video (or animated WebP) in the hero");
if (downloads.size < 3) bad(`index.html: ${downloads.size} download links (want at least 3)`);

// ---- images ---------------------------------------------------------------------------
for (const f of files.filter((f) => /\.(png|jpe?g|webp)$/i.test(f) && !rel(f).startsWith("app/"))) {
  const size = statSync(f).size;
  // The demo's animated WebP fallback has a cap of its own.
  const cap = /(^|\/)demo\.webp$/.test(rel(f)) ? MAX_ANIMATION : MAX_IMG;
  if (size > cap) bad(`${rel(f)}: ${Math.round(size / 1024)} KB (limit ${Math.round(cap / 1024)} KB)`);
}
for (const f of files.filter((f) => /\.(webm|mp4)$/i.test(f) && !rel(f).startsWith("app/"))) {
  const size = statSync(f).size;
  if (!/\.webm$/i.test(f)) bad(`${rel(f)}: the site's video is WebM only`);
  if (size > MAX_VIDEO) bad(`${rel(f)}: ${(size / 1024 / 1024).toFixed(1)} MB (limit 4 MB)`);
}

// ---- the demo -------------------------------------------------------------------------
if (existsSync(path.join(dir, "app", "index.html"))) {
  const app = readFileSync(path.join(dir, "app", "index.html"), "utf8");
  const scripts = [...app.matchAll(/<script[^>]*src="([^"]+)"/g)].map((m) => m[1]);
  if (!scripts.length) bad("app/index.html: no module script");
  // The demo's content security policy is the plugin sandbox's second lock (ADR-0009).
  const csp = (/<meta http-equiv="Content-Security-Policy" content="([^"]+)"/.exec(app)?.[1] ?? "").replace(/&#39;|&apos;/g, "'").replace(/&amp;/g, "&");
  if (!csp) bad("app/index.html: no Content-Security-Policy meta tag (vite.config.ts cspMeta)");
  else for (const want of ["script-src 'self' 'wasm-unsafe-eval' blob:", "font-src 'self' data:", "worker-src 'self' blob:", "object-src 'none'"]) if (!csp.includes(want)) bad(`app/index.html: the content security policy lacks "${want}"`);
  for (const s of scripts) if (!s.startsWith(`${base}app/assets/`)) bad(`app/index.html: script ${s} is not under ${base}app/assets/`);
  const assets = existsSync(path.join(dir, "app", "assets")) ? readdirSync(path.join(dir, "app", "assets")) : [];
  if (!assets.some((a) => a.endsWith(".wasm"))) bad("app/assets: no kernel .wasm (run npm run wasm before building the site)");
  if (!assets.some((a) => /kernel\.worker.*\.js$/.test(a))) bad("app/assets: no kernel worker");
  if (assets.some((a) => a.endsWith(".map"))) bad("app/assets: source maps in the public build");
  const bundle = assets.filter((a) => a.endsWith(".js")).map((a) => readFileSync(path.join(dir, "app", "assets", a), "utf8")).join("\n");
  if (!bundle.includes("Runs in your browser, nothing is uploaded.")) bad("app: the demo banner is not in the bundle (VITE_WEB_DEMO=1 missing?)");
}

// ---- identity: no machine paths or personal names -------------------------------------
for (const f of files.filter((f) => /\.(html|css|js|svg|txt|json)$/i.test(f) || path.basename(f) === "CNAME")) {
  const text = readFileSync(f, "utf8");
  const hit = /([A-Za-z]:\\\\?Users\\\\?[A-Za-z0-9_]+|\/home\/[a-z_][a-z0-9_-]*\/|\/Users\/[A-Za-z0-9_]+\/|\/mnt\/[a-z]\/)/.exec(text);
  if (hit) bad(`${rel(f)}: machine path ${hit[0]}`);
}

// ---- online: the download links answer ------------------------------------------------
if (online) {
  for (const url of downloads) {
    try {
      const r = await fetch(url, { method: "HEAD", redirect: "follow" });
      if (!r.ok) bad(`download ${url}: HTTP ${r.status}`);
    } catch (e) {
      bad(`download ${url}: ${e.message}`);
    }
  }
}

if (problems.length) {
  console.error(`[check-site] ${problems.length} problem(s):\n  - ${problems.join("\n  - ")}`);
  process.exit(1);
}
console.log(`[check-site] ok: ${pages.length} pages, ${files.length} files, base ${base}${online ? `, ${downloads.size} download links answer` : ""}`);
