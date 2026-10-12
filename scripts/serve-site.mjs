#!/usr/bin/env node
// Serve the assembled site (scripts/build-site.mjs) the way GitHub Pages does: directories
// answer with their index.html, a directory without its trailing slash redirects to it,
// and .wasm goes out as application/wasm (WebAssembly.instantiateStreaming needs it).
//
//   node scripts/serve-site.mjs [--dir dist-site] [--port 5214] [--prefix /]
//
// --prefix mounts the folder under a path, as the github.io project address does
// ("/sawhorse/"). Also importable: serveSite({ dir, port, prefix }) -> { url, close }.
import { createReadStream, existsSync, statSync } from "node:fs";
import { createServer } from "node:http";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".json": "application/json",
  ".wasm": "application/wasm",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".webp": "image/webp",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
  ".txt": "text/plain; charset=utf-8",
  ".ico": "image/x-icon",
};

export function serveSite({ dir, port = 5214, prefix = "/" }) {
  const rootDir = path.resolve(dir);
  let pre = prefix.startsWith("/") ? prefix : `/${prefix}`;
  if (!pre.endsWith("/")) pre = `${pre}/`;
  const server = createServer((req, res) => {
    const url = new URL(req.url ?? "/", "http://localhost");
    let p = decodeURIComponent(url.pathname);
    if (`${p}/` === pre) {
      res.writeHead(301, { Location: pre });
      return res.end();
    }
    if (!p.startsWith(pre)) {
      res.writeHead(404, { "Content-Type": "text/plain" });
      return res.end("not found");
    }
    p = p.slice(pre.length);
    const file = path.resolve(rootDir, p);
    if (file !== rootDir && !file.startsWith(rootDir + path.sep)) {
      res.writeHead(403);
      return res.end();
    }
    let target = file;
    if (existsSync(target) && statSync(target).isDirectory()) {
      if (!url.pathname.endsWith("/")) {
        res.writeHead(301, { Location: `${url.pathname}/${url.search}` });
        return res.end();
      }
      target = path.join(target, "index.html");
    }
    if (!existsSync(target) || !statSync(target).isFile()) {
      res.writeHead(404, { "Content-Type": "text/plain" });
      return res.end("not found");
    }
    res.writeHead(200, { "Content-Type": TYPES[path.extname(target).toLowerCase()] ?? "application/octet-stream", "Cache-Control": "no-store" });
    createReadStream(target).pipe(res);
  });
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", () =>
      resolve({
        url: `http://127.0.0.1:${port}${pre}`,
        close: () => new Promise((r) => server.close(() => r())),
      }),
    );
  });
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const args = Object.fromEntries(
    process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith("--") ? [...acc, [a.slice(2), all[i + 1]]] : acc), []),
  );
  const root = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
  const s = await serveSite({ dir: path.resolve(root, args.dir ?? "dist-site"), port: Number(args.port ?? 5214), prefix: args.prefix ?? "/" });
  console.log(`[serve] ${s.url}  (Ctrl+C to stop)`);
}
