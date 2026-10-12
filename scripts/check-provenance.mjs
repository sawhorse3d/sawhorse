#!/usr/bin/env node
// Provenance tripwire for a built output directory (default: dist).
//
// Refuses the build when it contains:
//   1. any source map (*.map) or an inline sourceMappingURL data: URI, because a map embeds
//      every dependency's original source, which is how the SketchUp-authored blank template
//      inside npm openskp's writer ended up in the v0.1.0-alpha.1 installers; or
//   2. the bytes of a SketchUp document header ("SketchUp Model" in UTF-16LE, or the VFF zip
//      marker), raw, inside a base64 literal, or inside a gzip/deflate stream.
//
// Usage: node scripts/check-provenance.mjs [dir ...]   (exit 1 with a list of offenders)
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { gunzipSync, inflateRawSync, inflateSync } from "node:zlib";

const dirs = process.argv.slice(2).length ? process.argv.slice(2) : ["dist"];
const HEADER = Buffer.from("SketchUp Model", "utf16le");
const VFF = Buffer.from("VFF");
const BASE64 = /[A-Za-z0-9+/=]{1500,}/g;

function hasSkpBytes(buf) {
  if (buf.includes(HEADER)) return "utf16 header";
  if (buf.length > 4 && buf[0] === 0x50 && buf[1] === 0x4b && buf.subarray(0, 4096).includes(VFF)) return "VFF container";
  return null;
}

function inflateAny(buf) {
  for (const fn of [gunzipSync, inflateSync, inflateRawSync]) {
    try {
      return fn(buf);
    } catch {
      /* not that encoding */
    }
  }
  return null;
}

function scanFile(path) {
  const buf = readFileSync(path);
  const findings = [];
  if (/\.map$/i.test(path)) findings.push("source map file");
  if (/\.(js|mjs|cjs|css)$/i.test(path) && buf.includes(Buffer.from("sourceMappingURL=data:"))) findings.push("inline source map");
  const raw = hasSkpBytes(buf);
  if (raw) findings.push(`SketchUp bytes (${raw})`);
  const inflated = inflateAny(buf);
  if (inflated) {
    const r = hasSkpBytes(inflated);
    if (r) findings.push(`SketchUp bytes inside compressed stream (${r})`);
  }
  if (/\.(js|mjs|cjs|map|json|html|txt)$/i.test(path)) {
    const text = buf.toString("latin1");
    for (const m of text.matchAll(BASE64)) {
      let decoded;
      try {
        decoded = Buffer.from(m[0], "base64");
      } catch {
        continue;
      }
      const r = hasSkpBytes(decoded) || (inflateAny(decoded) && hasSkpBytes(inflateAny(decoded)));
      if (r) {
        findings.push(`SketchUp bytes inside a base64 literal (${decoded.length} bytes, ${r})`);
        break;
      }
    }
  }
  return findings;
}

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) walk(p, out);
    else if (st.size <= 64 * 1024 * 1024) out.push(p);
  }
  return out;
}

let bad = 0;
let scanned = 0;
for (const dir of dirs) {
  let files;
  try {
    files = walk(dir);
  } catch (err) {
    console.error(`check-provenance: cannot read ${dir}: ${err.message}`);
    process.exit(2);
  }
  for (const f of files) {
    scanned += 1;
    const findings = scanFile(f);
    if (findings.length) {
      bad += 1;
      console.error(`check-provenance: ${relative(process.cwd(), f)}: ${findings.join("; ")}`);
    }
  }
}
if (bad) {
  console.error(`check-provenance: ${bad} offending file(s) in ${dirs.join(", ")} (${scanned} scanned). Build without source maps (SAWHORSE_SOURCEMAP unset) and keep SketchUp-authored bytes out of the tree.`);
  process.exit(1);
}
console.log(`check-provenance: ok, ${scanned} files in ${dirs.join(", ")}: no source maps, no SketchUp document bytes`);
