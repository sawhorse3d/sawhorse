// From files to a plugin the app can list and start: the manifest checked, every module
// reachable from the entry checked (packages/plugin-api: the same rules plugin:check
// applies), and only those modules kept for the sandbox.
import { parseManifest, type Manifest } from "../../packages/plugin-api/src/manifest.ts";
import { checkPackage } from "../../packages/plugin-api/src/source.ts";

/** Where a plugin came from. */
export type PluginSource =
  /** Shipped with the app (plugins/ in the repository): listed, disabled until enabled. */
  | "bundled"
  /** Installed from a .zip: kept in app data (desktop) or this browser. */
  | "installed"
  /** A folder on disk (desktop, developers): read again on Reload. */
  | "unpacked"
  /** A .zip loaded for this session only (browser). */
  | "session";

export interface PluginPackage {
  manifest: Manifest;
  /** Every text file (modules, plugin.json, README.md ...). */
  files: Record<string, string>;
  /** The modules the sandbox loads, in an order where each comes after its imports. */
  modules: string[];
  source: PluginSource;
  /** The folder on disk (desktop installed and unpacked plugins). */
  location?: string;
}

export type LoadResult = { ok: true; pkg: PluginPackage } | { ok: false; id: string | null; name: string; errors: string[] };

/** Check a plugin's files and make a package of them. */
export function loadPackage(files: Record<string, string>, source: PluginSource, location?: string): LoadResult {
  const raw = files["plugin.json"];
  if (raw === undefined) return { ok: false, id: null, name: location ?? "plugin", errors: ["there is no plugin.json at the top of the plugin folder"] };
  const r = parseManifest(raw);
  if (!r.ok) {
    let id: string | null = null;
    let name = location ?? "plugin";
    try {
      const j = JSON.parse(raw) as { id?: unknown; name?: unknown };
      if (typeof j.id === "string") id = j.id.slice(0, 100);
      if (typeof j.name === "string") name = j.name.slice(0, 64);
    } catch {
      /* the errors say why */
    }
    return { ok: false, id, name, errors: r.errors };
  }
  const check = checkPackage(files, r.manifest.entry);
  if (check.problems.length) return { ok: false, id: r.manifest.id, name: r.manifest.name, errors: check.problems };
  const pkg: PluginPackage = { manifest: r.manifest, files, modules: check.order, source };
  if (location) pkg.location = location;
  return { ok: true, pkg };
}

/** Only the modules the sandbox needs. */
export function moduleFiles(pkg: PluginPackage): Record<string, string> {
  return Object.fromEntries(pkg.modules.map((m) => [m, pkg.files[m]]));
}
