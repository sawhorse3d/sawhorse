// Lane 16: checking a plugin package off the app's thread. Parsing a package's modules and
// digesting its files is linear in their size, but at the package limits that is seconds of
// a frozen window, at install and again at every start for an installed plugin. A package
// of more than CHECK_IN_WORKER characters is checked in a worker (check.worker.ts); a small
// one (the bundled samples, tests) right here.
import { parseManifest } from "../../packages/plugin-api/src/manifest.ts";
import type { PackageCheck } from "../../packages/plugin-api/src/source.ts";
import { loadPackage, type LoadResult, type PluginSource } from "./loader";
import { packageDigest } from "./store";
import { PackageError } from "./zip";

/** Characters of a package's files above which it is checked in a worker. */
export const CHECK_IN_WORKER = 256 * 1024;
/** How long a check may take (a module crafted to make the parser slow is refused). */
export const CHECK_SECONDS = 60;

/** A package checked: what loadPackage says, and its digest (empty when it is not usable). */
export interface Prepared {
  result: LoadResult;
  digest: string;
}

const size = (files: Record<string, string>) => Object.values(files).reduce((n, t) => n + t.length, 0);

let worker: Worker | null = null;
let next = 1;
const waiting = new Map<number, { resolve: (v: { check: PackageCheck | null; digest: string }) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }>();

function checkWorker(): Worker | null {
  if (typeof Worker === "undefined") return null;
  if (worker) return worker;
  try {
    worker = new Worker(new URL("./check.worker.ts", import.meta.url), { type: "module", name: "sawhorse-plugin-check" });
  } catch {
    return null;
  }
  worker.onmessage = (e: MessageEvent<{ id: number; ok: boolean; check?: PackageCheck | null; digest?: string; error?: string }>) => {
    const w = waiting.get(e.data.id);
    if (!w) return;
    waiting.delete(e.data.id);
    clearTimeout(w.timer);
    if (e.data.ok) w.resolve({ check: e.data.check ?? null, digest: e.data.digest ?? "" });
    else w.reject(new PackageError(e.data.error ?? "the plugin's code could not be checked"));
  };
  worker.onerror = () => {
    // The checker itself failed: every waiting check is refused, the next one starts afresh.
    for (const w of waiting.values()) {
      clearTimeout(w.timer);
      w.reject(new PackageError("the plugin's code could not be checked"));
    }
    waiting.clear();
    worker?.terminate();
    worker = null;
  };
  return worker;
}

function inWorker(w: Worker, files: Record<string, string>, entry: string | null): Promise<{ check: PackageCheck | null; digest: string }> {
  const id = next++;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      waiting.delete(id);
      // A check that runs away takes its worker with it.
      w.terminate();
      if (worker === w) worker = null;
      for (const o of waiting.values()) {
        clearTimeout(o.timer);
        o.reject(new PackageError("the plugin's code could not be checked"));
      }
      waiting.clear();
      reject(new PackageError(`checking the plugin's code took more than ${CHECK_SECONDS} s`));
    }, CHECK_SECONDS * 1000);
    waiting.set(id, { resolve, reject, timer });
    w.postMessage({ id, files, entry });
  });
}

/** Check `files` as a plugin package: off the app's thread when the package is large. */
export async function preparePackage(files: Record<string, string>, source: PluginSource, location?: string): Promise<Prepared> {
  const w = size(files) > CHECK_IN_WORKER ? checkWorker() : null;
  if (!w) {
    const result = loadPackage(files, source, location);
    return { result, digest: result.ok ? packageDigest(result.pkg.files) : "" };
  }
  // The manifest is small and read here; the modules are parsed in the worker.
  const raw = files["plugin.json"];
  const m = raw === undefined ? null : parseManifest(raw);
  const entry = m?.ok ? m.manifest.entry : null;
  const { check, digest } = await inWorker(w, files, entry);
  const result = loadPackage(files, source, location, check ?? undefined);
  return { result, digest: result.ok ? digest : "" };
}
