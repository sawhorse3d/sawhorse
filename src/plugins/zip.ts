// Plugin packages as files: a .zip of the plugin folder (Install from .zip, Load from .zip)
// or a folder read from disk (desktop). fflate (MIT, pure JS, already in the build for
// 3MF and .shz checks) does the zip work.
import { strFromU8, strToU8, unzipSync, zipSync } from "fflate";

/** Most files and bytes a plugin package may hold (a zip bomb stops at the limit). */
export const PACKAGE_LIMITS = { files: 2000, bytes: 50 * 1024 * 1024, fileBytes: 8 * 1024 * 1024 };

/** Files read as text (the rest are listed by name only). */
const TEXT = /\.(m?js|json|md|txt)$|(^|\/)(LICEN[CS]E|NOTICE|COPYING)$/i;

export class PackageError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PackageError";
  }
}

/** Skip folders, dotfiles and OS clutter. */
function skipped(path: string): boolean {
  return path.endsWith("/") || path.split("/").some((p) => (p.startsWith(".") && p !== "..") || p === "__MACOSX" || p === "node_modules");
}

/**
 * The files of a plugin package, keyed by their path inside the plugin folder. A zip that
 * holds one top-level folder (zipping the folder rather than its contents) is unwrapped.
 * Text files come back as strings; other files are listed with their size.
 */
export function filesFromEntries(entries: Record<string, Uint8Array>): { files: Record<string, string>; other: Record<string, number> } {
  const names = Object.keys(entries)
    .map((n) => n.replace(/\\/g, "/"))
    .filter((n) => !skipped(n));
  if (names.length > PACKAGE_LIMITS.files) throw new PackageError(`the package holds more than ${PACKAGE_LIMITS.files} files`);
  for (const n of names) if (n.startsWith("/") || n.split("/").includes("..")) throw new PackageError(`the package has a file outside its folder (${n})`);
  // One folder around everything (and no plugin.json at the top): look inside it.
  let prefix = "";
  if (!names.includes("plugin.json")) {
    const tops = new Set(names.map((n) => n.split("/")[0]));
    if (tops.size === 1) {
      const top = [...tops][0];
      if (names.every((n) => n.startsWith(`${top}/`))) prefix = `${top}/`;
    }
  }
  const files: Record<string, string> = {};
  const other: Record<string, number> = {};
  let total = 0;
  for (const [raw, bytes] of Object.entries(entries)) {
    const n = raw.replace(/\\/g, "/");
    if (skipped(n) || !n.startsWith(prefix)) continue;
    const path = n.slice(prefix.length);
    if (!path) continue;
    total += bytes.byteLength;
    if (bytes.byteLength > PACKAGE_LIMITS.fileBytes) throw new PackageError(`${path} is larger than ${PACKAGE_LIMITS.fileBytes / 1024 / 1024} MB`);
    if (total > PACKAGE_LIMITS.bytes) throw new PackageError(`the package is larger than ${PACKAGE_LIMITS.bytes / 1024 / 1024} MB`);
    if (TEXT.test(path)) files[path] = strFromU8(bytes);
    else other[path] = bytes.byteLength;
  }
  return { files, other };
}

/** Read a .zip of a plugin folder. */
export function readPluginZip(bytes: Uint8Array): { files: Record<string, string>; other: Record<string, number> } {
  let count = 0;
  let total = 0;
  let entries: Record<string, Uint8Array>;
  try {
    entries = unzipSync(bytes, {
      filter: (f) => {
        if (skipped(f.name)) return false;
        count++;
        total += f.originalSize;
        if (count > PACKAGE_LIMITS.files) throw new PackageError(`the zip holds more than ${PACKAGE_LIMITS.files} files`);
        if (total > PACKAGE_LIMITS.bytes) throw new PackageError(`the zip unpacks to more than ${PACKAGE_LIMITS.bytes / 1024 / 1024} MB`);
        return true;
      },
    });
  } catch (e) {
    if (e instanceof PackageError) throw e;
    throw new PackageError(`not a readable .zip (${e instanceof Error ? e.message : String(e)})`);
  }
  return filesFromEntries(entries);
}

/** Zip a plugin's text files (plugin.json first), as plugin:pack does. */
export function writePluginZip(files: Record<string, string>): Uint8Array {
  const names = Object.keys(files).sort((a, b) => (a === "plugin.json" ? -1 : b === "plugin.json" ? 1 : a.localeCompare(b)));
  const mtime = new Date(Date.UTC(2026, 0, 1));
  return zipSync(Object.fromEntries(names.map((n) => [n, [strToU8(files[n]), { mtime, level: 9 }]])));
}
