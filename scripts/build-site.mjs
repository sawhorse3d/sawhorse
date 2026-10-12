#!/usr/bin/env node
// Assemble the public site (https://sawhorse3d.com) into one static folder:
//
//   <out>/            site/ (the landing page, its stylesheet and screenshots)
//   <out>/app/        the browser demo: the Vite app built with VITE_BASE=<base>app/ and
//                     VITE_WEB_DEMO=1 (the demo banner, the Help menu)
//   <out>/guide/      docs/USER-GUIDE.md rendered to HTML
//   <out>/changelog/  CHANGELOG.md rendered to HTML
//   <out>/CNAME       sawhorse3d.com
//   <out>/.nojekyll   serve every file as is
//
//   node scripts/build-site.mjs [--out dist-site] [--base /] [--skip-app]
//
// --base is the path the site is served under: "/" on the custom domain, "/sawhorse/" on
// the github.io project address (.github/workflows/pages.yml passes what
// actions/configure-pages reports). The kernel WASM must already be built (npm run wasm);
// without it the demo would open with "kernel not built", so this refuses.
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Marked } from "marked";

const root = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith("--") ? [...acc, [a.slice(2), all[i + 1]?.startsWith("--") || all[i + 1] === undefined ? "1" : all[i + 1]]] : acc), []),
);
const out = path.resolve(root, args.out ?? "dist-site");
let base = args.base && args.base !== "1" ? args.base : "/";
if (!base.startsWith("/")) base = `/${base}`;
if (!base.endsWith("/")) base = `${base}/`;
const DOMAIN = "sawhorse3d.com";
const REPO = "https://github.com/sawhorse3d/sawhorse";

function step(msg) {
  console.log(`[site] ${msg}`);
}

// ---- 1. the static landing page -------------------------------------------------------
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
cpSync(path.join(root, "site"), out, { recursive: true });
step(`copied site/ -> ${path.relative(root, out)}/`);

// ---- 2. the browser demo --------------------------------------------------------------
if (args["skip-app"] !== "1") {
  if (!existsSync(path.join(root, "src", "wasm", "pkg", "sawhorse_wasm_bg.wasm"))) {
    console.error("[site] the kernel WASM is not built (src/wasm/pkg). Run: npm run wasm");
    process.exit(1);
  }
  const appBase = `${base}app/`;
  step(`vite build --base ${appBase} (VITE_WEB_DEMO=1)`);
  const vite = path.join(root, "node_modules", "vite", "bin", "vite.js");
  const r = spawnSync(process.execPath, [vite, "build", "--outDir", path.join(out, "app"), "--emptyOutDir"], {
    cwd: root,
    stdio: "inherit",
    env: { ...process.env, VITE_BASE: appBase, VITE_WEB_DEMO: "1" },
  });
  if (r.status !== 0) process.exit(r.status ?? 1);
}

// ---- 3. documents ---------------------------------------------------------------------
const slug = (text) =>
  text
    .toLowerCase()
    .replace(/<[^>]+>/g, "")
    .replace(/&[a-z]+;/g, "")
    .replace(/[^a-z0-9 -]/g, "")
    .trim()
    .replace(/\s+/g, "-");

const escapeHtml = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

/** Markdown to HTML with anchored headings; relative links go to the file on GitHub. */
function renderMarkdown(md, { docPath }) {
  const seen = new Map();
  const toc = [];
  const marked = new Marked({
    gfm: true,
    renderer: {
      heading({ tokens, depth }) {
        const text = this.parser.parseInline(tokens);
        let id = slug(text) || "section";
        const n = seen.get(id) ?? 0;
        seen.set(id, n + 1);
        if (n) id = `${id}-${n}`;
        if (depth === 2) toc.push({ id, text });
        const anchor = depth > 1 ? ` <a class="anchor" href="#${id}" aria-label="Link to this section">#</a>` : "";
        return `<h${depth} id="${id}">${text}${anchor}</h${depth}>\n`;
      },
      link({ href, title, tokens }) {
        const text = this.parser.parseInline(tokens);
        let url = href;
        if (!/^([a-z]+:|#|\/)/i.test(url)) {
          // Repository-relative (docs/format.md, ../crates/...): link the file on GitHub.
          const resolved = path.posix.normalize(path.posix.join(path.posix.dirname(docPath), url));
          url = `${REPO}/blob/main/${resolved}`;
        }
        const t = title ? ` title="${escapeHtml(title)}"` : "";
        return `<a href="${escapeHtml(url)}"${t}>${text}</a>`;
      },
      // Raw HTML in our docs is not expected; escape it rather than pass it through.
      html({ text }) {
        return escapeHtml(text);
      },
    },
  });
  const body = marked.parse(md);
  return { body, toc };
}

function page({ title, description, body, toc, source }) {
  const tocHtml = toc.length
    ? `<nav class="toc" aria-label="On this page"><b>On this page</b><ul>${toc.map((t) => `<li><a href="#${t.id}">${t.text}</a></li>`).join("")}</ul></nav>`
    : "";
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escapeHtml(title)}</title>
<meta name="description" content="${escapeHtml(description)}">
<meta name="color-scheme" content="light dark">
<link rel="icon" type="image/svg+xml" href="../favicon.svg">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Barlow+Condensed:wght@500;600&amp;family=IBM+Plex+Sans:wght@400;500;600&amp;family=IBM+Plex+Mono:wght@400;500&amp;display=swap">
<link rel="stylesheet" href="../style.css">
</head>
<body>
<header class="top">
  <div class="wrap">
    <a class="brand" href="../" aria-label="Sawhorse home">
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 9h18M5 9l-2 11M19 9l2 11M8 9l-1.5 11M16 9l1.5 11M6.5 15h11" stroke="var(--accent)" stroke-width="1.8" fill="none" stroke-linecap="round"/></svg>
      Sawhorse
    </a>
    <nav aria-label="Site">
      <a href="../app/">Try it</a>
      <a href="../#download">Download</a>
      <a href="../guide/">User guide</a>
      <a href="../changelog/">Changelog</a>
      <a href="${REPO}">GitHub</a>
    </nav>
  </div>
</header>
<main class="wrap doc">
${tocHtml}
${body}
<p class="back"><a href="${REPO}/blob/main/${source}">This page on GitHub</a> · <a href="../">Sawhorse home</a></p>
</main>
<footer>
  <div class="wrap">
    <p>Sawhorse is free software, MIT OR Apache-2.0. maintained by @boardfoot</p>
    <p>Not affiliated with Trimble. SketchUp is a trademark of Trimble Inc.</p>
  </div>
</footer>
</body>
</html>
`;
}

const docs = [
  { src: "docs/USER-GUIDE.md", dir: "guide", title: "User guide · Sawhorse", description: "How to use Sawhorse: the first ten minutes, every key, inference colours, the transform tools, groups and components, the cut list and nesting.", toc: true },
  { src: "CHANGELOG.md", dir: "changelog", title: "Changelog · Sawhorse", description: "Every notable change to Sawhorse, newest first.", toc: false },
];
for (const d of docs) {
  const md = readFileSync(path.join(root, d.src), "utf8");
  const { body, toc } = renderMarkdown(md, { docPath: d.src });
  mkdirSync(path.join(out, d.dir), { recursive: true });
  writeFileSync(path.join(out, d.dir, "index.html"), page({ title: d.title, description: d.description, body, toc: d.toc ? toc : [], source: d.src }));
  step(`${d.src} -> ${d.dir}/index.html`);
}

// ---- 4. Pages plumbing ----------------------------------------------------------------
writeFileSync(path.join(out, "CNAME"), `${DOMAIN}\n`);
writeFileSync(path.join(out, ".nojekyll"), "");
step(`CNAME (${DOMAIN}), .nojekyll; site base ${base}`);
step(`done: ${path.relative(root, out) || "."}`);
