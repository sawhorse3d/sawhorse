// Where installed plugins live, per host:
//
// - Desktop (Tauri): folders under <app data>/plugins/<id>/, read on start. Install from
//   .zip writes one; Uninstall removes it; "Load unpacked folder" reads a folder the person
//   picks (read again on Reload, never copied); "Open folder" shows it in the file manager.
//   The fs plugin's scope allows only $APPDATA (src-tauri/capabilities/default.json) and the
//   folders picked in a dialog.
// - Browser: installed packages in IndexedDB (store.ts); a .zip can also be loaded for this
//   session only.
import { isTauri } from "../io/files";
import { browserPackages } from "./store";
import { BAD_SEGMENT, filesFromEntries, PACKAGE_LIMITS, PackageError } from "./zip";

export interface InstalledFolder {
  files: Record<string, string>;
  /** The folder (desktop). */
  location?: string;
}

export interface PluginPlatform {
  kind: "desktop" | "browser";
  /** Every installed plugin's files. */
  list(): Promise<InstalledFolder[]>;
  /** Keep a plugin's files; returns its folder on desktop. */
  install(id: string, files: Record<string, string>): Promise<string | undefined>;
  uninstall(id: string, location?: string): Promise<void>;
  /** Desktop: pick a plugin folder and read it. */
  pickFolder?(): Promise<InstalledFolder | null>;
  /** Desktop: read a folder again (Reload of an unpacked plugin). */
  readFolder?(location: string): Promise<Record<string, string>>;
  /** Desktop: show the folder in the file manager. */
  openFolder?(location: string): Promise<void>;
}

const TEXT = /\.(m?js|json|md|txt)$|(^|\/)(LICEN[CS]E|NOTICE|COPYING)$/i;

async function tauriFs() {
  return import("@tauri-apps/plugin-fs");
}

async function pluginsRoot(): Promise<string> {
  const { appDataDir, join } = await import("@tauri-apps/api/path");
  return join(await appDataDir(), "plugins");
}

/** Every text file under a folder (skipping dotfiles and node_modules), keyed by relative path. */
async function readTree(dir: string): Promise<Record<string, string>> {
  const fs = await tauriFs();
  const { join } = await import("@tauri-apps/api/path");
  const entries: Record<string, Uint8Array> = {};
  let count = 0;
  let total = 0;
  const walk = async (d: string, prefix: string, depth: number) => {
    if (depth > 32) throw new PackageError("the plugin folder nests more than 32 folders deep");
    for (const e of await fs.readDir(d)) {
      if (e.name.startsWith(".") || e.name === "node_modules") continue;
      if (++count > PACKAGE_LIMITS.files) throw new PackageError(`the plugin folder holds more than ${PACKAGE_LIMITS.files} files`);
      const full = await join(d, e.name);
      if (e.isDirectory) await walk(full, `${prefix}${e.name}/`, depth + 1);
      else if (e.isFile && TEXT.test(e.name)) {
        // Measured before it is read (lane 16).
        const size = (await fs.stat(full)).size;
        total += size;
        if (size > PACKAGE_LIMITS.fileBytes) throw new PackageError(`${prefix}${e.name} is larger than ${PACKAGE_LIMITS.fileBytes / 1024 / 1024} MB`);
        if (total > PACKAGE_LIMITS.bytes) throw new PackageError(`the plugin folder is larger than ${PACKAGE_LIMITS.bytes / 1024 / 1024} MB`);
        entries[`${prefix}${e.name}`] = await fs.readFile(full);
      }
    }
  };
  await walk(dir, "", 0);
  return filesFromEntries(entries).files;
}

function desktop(): PluginPlatform {
  return {
    kind: "desktop",
    async list() {
      const fs = await tauriFs();
      const root = await pluginsRoot();
      if (!(await fs.exists(root))) return [];
      const { join } = await import("@tauri-apps/api/path");
      const out: InstalledFolder[] = [];
      for (const e of await fs.readDir(root)) {
        if (!e.isDirectory || e.name.startsWith(".")) continue;
        const location = await join(root, e.name);
        try {
          out.push({ files: await readTree(location), location });
        } catch {
          out.push({ files: {}, location });
        }
      }
      return out;
    },
    async install(id, files) {
      const fs = await tauriFs();
      const { join, dirname, sep } = await import("@tauri-apps/api/path");
      const root = await pluginsRoot();
      const dir = await join(root, id);
      // Lane 16: written into a staging folder first, then swapped in, so a failure halfway
      // leaves the installed version as it was; every target is checked to be inside it.
      const stamp = `${Date.now()}-${Math.floor(Math.random() * 1e9)}`;
      const staging = await join(root, `.staging-${id}-${stamp}`);
      const s = sep();
      const inside = (base: string, p: string) => p.startsWith(base.endsWith(s) ? base : `${base}${s}`);
      try {
        await fs.mkdir(staging, { recursive: true });
        for (const [rel, text] of Object.entries(files)) {
          const segs = rel.split("/");
          if (segs.some((x) => !x || x === "." || x === ".." || BAD_SEGMENT.test(x))) throw new PackageError(`not a plain file name: ${rel.slice(0, 120)}`);
          const target = await join(staging, ...segs);
          if (!inside(staging, target)) throw new PackageError(`${rel.slice(0, 120)} would be written outside the plugin's folder`);
          await fs.mkdir(await dirname(target), { recursive: true });
          await fs.writeTextFile(target, text);
        }
        const old = (await fs.exists(dir)) ? await join(root, `.old-${id}-${stamp}`) : null;
        if (old) await fs.rename(dir, old);
        try {
          await fs.rename(staging, dir);
        } catch (e) {
          if (old) await fs.rename(old, dir);
          throw e;
        }
        if (old) await fs.remove(old, { recursive: true }).catch(() => undefined);
        return dir;
      } catch (e) {
        await fs.remove(staging, { recursive: true }).catch(() => undefined);
        throw e;
      }
    },
    async uninstall(id, location) {
      const fs = await tauriFs();
      const { join } = await import("@tauri-apps/api/path");
      const root = await pluginsRoot();
      const dir = location ?? (await join(root, id));
      // Only ever a folder inside <app data>/plugins.
      if (!dir.startsWith(root)) throw new Error("that plugin is not in the app's plugin folder");
      if (await fs.exists(dir)) await fs.remove(dir, { recursive: true });
    },
    async pickFolder() {
      const { open } = await import("@tauri-apps/plugin-dialog");
      // recursive: the dialog adds the folder and everything under it to the fs scope.
      const dir = await open({ directory: true, multiple: false, recursive: true, title: "Load an unpacked plugin folder" });
      if (typeof dir !== "string") return null;
      return { files: await readTree(dir), location: dir };
    },
    readFolder: (location) => readTree(location),
    async openFolder(location) {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("open_plugin_folder", { path: location });
    },
  };
}

function browser(): PluginPlatform {
  return {
    kind: "browser",
    async list() {
      return (await browserPackages.list()).map((files) => ({ files }));
    },
    async install(id, files) {
      if (!(await browserPackages.put(id, files))) throw new Error("this browser is not keeping data for the app (private window or storage blocked): load the plugin for this session instead");
      return undefined;
    },
    async uninstall(id) {
      await browserPackages.remove(id);
    },
  };
}

export function pluginPlatform(): PluginPlatform {
  return isTauri() ? desktop() : browser();
}
