/// <reference lib="webworker" />
// Lane 16: a plugin package's modules are parsed (acorn) and its files digested (SHA-256)
// here, off the app's thread: a package at its limits (16 MB of modules, 50 MB of files)
// takes seconds to check, which used to freeze the window at install and at every start.
// The app's checker (checker.ts) sends the files and the entry and gets the check back.
import { checkPackage } from "../../packages/plugin-api/src/source.ts";
import { packageDigest } from "./store";

const ctx = self as unknown as DedicatedWorkerGlobalScope;

ctx.onmessage = (e: MessageEvent<{ id: number; files: Record<string, string>; entry: string | null }>) => {
  const { id, files, entry } = e.data;
  try {
    const check = entry === null ? null : checkPackage(files, entry);
    ctx.postMessage({ id, ok: true, check, digest: packageDigest(files) });
  } catch (err) {
    ctx.postMessage({ id, ok: false, error: err instanceof Error ? err.message : String(err) });
  }
};
