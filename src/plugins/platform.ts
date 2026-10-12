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
import { filesFromEntries } from "./zip";

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

const TEXT = /\.(m?js|json|md|txt)$/i;

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
  const walk = async (d: string, prefix: string) => {
    for (const e of await fs.readDir(d)) {
      if (e.name.startsWith(".") || e.name === "node_modules") continue;
      if (++count > 2000) throw new Error("the plugin folder holds more than 2,000 files");
      const full = await join(d, e.name);
      if (e.isDirectory) await walk(full, `${prefix}${e.name}/`);
      else if (e.isFile && TEXT.test(e.name)) entries[`${prefix}${e.name}`] = await fs.readFile(full);
    }
  };
  await walk(dir, "");
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
      const { join, dirname } = await import("@tauri-apps/api/path");
      const dir = await join(await pluginsRoot(), id);
      if (await fs.exists(dir)) await fs.remove(dir, { recursive: true });
      await fs.mkdir(dir, { recursive: true });
      for (const [rel, text] of Object.entries(files)) {
        const target = await join(dir, ...rel.split("/"));
        await fs.mkdir(await dirname(target), { recursive: true });
        await fs.writeTextFile(target, text);
      }
      return dir;
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
      const dir = await open({ directory: true, multiple: false, title: "Load an unpacked plugin folder" });
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
