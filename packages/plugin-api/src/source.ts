// What a plugin's code may contain, checked before it runs (by the app's loader, again by
// the sandbox as it links the modules, and by `npm run plugin:check`):
//
// - ES modules only. A module imports "sawhorse" (the API) or another module of the same
//   plugin by a relative path ("./lib/shelves.js"). Nothing else: no URLs, no packages, no
//   files outside the plugin folder.
// - No dynamic `import(...)` (nor any other `import.<phase>` form) and no `importScripts`:
//   everything a plugin runs is in its own folder and was checked here. (The sandbox also
//   removes importScripts and every way to turn a string into code, and runs under the
//   app's content security policy, so this is the first of several locks, not the only one.)
// - Modules may not import each other in a circle (each module becomes a blob URL that
//   must exist before the modules importing it).
//
// Every module is parsed with a real ECMAScript parser (acorn), as a module, exactly the way
// the browser will read it, and the checks look at the syntax tree. An earlier version
// scanned the text with comments and strings blanked out; it had to guess whether a `/`
// started a regular expression or was a division, and a regex placed where it guessed wrong
// (after `if (x)`, `throw`, `delete`, `export default` ...) hid an `import(...)` from it.
// A parser does not guess. A module the parser cannot read is refused (it would not load
// either, or it uses syntax too new to check).
import { parse } from "acorn";

export interface ImportRef {
  /** The specifier, as the module loader reads it (escapes resolved). */
  spec: string;
  /** Where the specifier's text starts and ends in the source (inside the quotes). */
  start: number;
  end: number;
}

export interface ModuleScan {
  imports: ImportRef[];
  problems: string[];
}

/** The API module's name. */
export const API_MODULE = "sawhorse";

/** A node of acorn's syntax tree, looked at generically. */
type AstNode = { type: string; start: number; end: number } & Record<string, unknown>;

const isNode = (v: unknown): v is AstNode => typeof v === "object" && v !== null && typeof (v as { type?: unknown }).type === "string" && typeof (v as { start?: unknown }).start === "number";

/** Every node under `root` (depth first, any order). */
function eachNode(root: AstNode, visit: (n: AstNode) => void): void {
  const stack: AstNode[] = [root];
  while (stack.length) {
    const n = stack.pop()!;
    visit(n);
    for (const key of Object.keys(n)) {
      if (key === "type" || key === "start" || key === "end" || key === "loc" || key === "range") continue;
      const v = n[key];
      if (Array.isArray(v)) {
        for (const c of v) if (isNode(c)) stack.push(c);
      } else if (isNode(v)) stack.push(v);
    }
  }
}

function lineOf(src: string, at: number): number {
  let n = 1;
  for (let k = 0; k < at && k < src.length; k++) if (src[k] === "\n") n++;
  return n;
}

/** Parse one module the way a browser loads it (module goal: strict, top-level await, no HTML comments). */
export function parseModule(src: string): AstNode {
  return parse(src, { ecmaVersion: "latest", sourceType: "module", allowHashBang: true }) as unknown as AstNode;
}

/** The imports of one module and anything in it that is not allowed. */
export function scanModule(path: string, src: string): ModuleScan {
  const problems: string[] = [];
  const imports: ImportRef[] = [];
  let program: AstNode;
  try {
    program = parseModule(src);
  } catch (e) {
    const err = e as { message?: string; loc?: { line?: number } };
    const line = err.loc?.line ?? 1;
    const message = String(err.message ?? e).replace(/\s*\(\d+:\d+\)$/, "");
    return { imports, problems: [`${path}:${line}: cannot be read as a JavaScript module: ${message}`] };
  }
  const at = (n: AstNode) => `${path}:${lineOf(src, n.start)}`;
  eachNode(program, (n) => {
    switch (n.type) {
      case "ImportExpression":
        problems.push(`${at(n)}: dynamic import() is not allowed in a plugin (import modules statically from the plugin's own folder)`);
        break;
      case "MetaProperty": {
        const meta = n.meta as { name?: string } | undefined;
        const prop = n.property as { name?: string } | undefined;
        if (meta?.name === "import" && prop?.name !== "meta") problems.push(`${at(n)}: import.${prop?.name ?? "?"} is not allowed in a plugin (import modules statically from the plugin's own folder)`);
        break;
      }
      case "Identifier":
      case "PrivateIdentifier":
        if (n.name === "importScripts") problems.push(`${at(n)}: importScripts is not available to plugins`);
        break;
      case "ImportDeclaration":
      case "ExportAllDeclaration":
      case "ExportNamedDeclaration": {
        const source = n.source;
        if (!isNode(source)) break;
        if (source.type !== "Literal" || typeof source.value !== "string") {
          problems.push(`${at(n)}: an import names its module with a plain string`);
          break;
        }
        // Inside the quotes; the sandbox replaces exactly this text with the module's URL.
        imports.push({ spec: source.value, start: source.start + 1, end: source.end - 1 });
        break;
      }
    }
  });
  // In source order (the walk is depth first from the end).
  imports.sort((a, b) => a.start - b.start);
  const order = (p: string) => Number(/:(\d+):/.exec(p)?.[1] ?? 0);
  problems.sort((a, b) => order(a) - order(b));
  return { imports, problems };
}

/** A path inside the plugin, normalised ("a/./b/../c.js" -> "a/c.js"); null when it leaves the folder. */
export function normalisePath(path: string): string | null {
  const out: string[] = [];
  for (const part of path.replace(/\\/g, "/").split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      if (!out.length) return null;
      out.pop();
    } else out.push(part);
  }
  return out.join("/");
}

/** What `spec`, imported by the module at `from`, refers to: the API, a module of the plugin, or a problem. */
export function resolveSpecifier(from: string, spec: string, files: Record<string, string>): { api: true } | { path: string } | { problem: string } {
  if (spec === API_MODULE) return { api: true };
  if (!spec.startsWith("./") && !spec.startsWith("../")) {
    return { problem: `${from} imports "${spec}": a plugin may import only "sawhorse" and its own files ("./name.js")` };
  }
  const dir = from.includes("/") ? from.slice(0, from.lastIndexOf("/") + 1) : "";
  const path = normalisePath(dir + spec);
  if (path === null) return { problem: `${from} imports "${spec}", which is outside the plugin folder` };
  if (!/\.m?js$/.test(path)) return { problem: `${from} imports "${spec}": name the file with its .js extension` };
  if (!(path in files)) return { problem: `${from} imports "${spec}", but the plugin has no ${path}` };
  return { path };
}

export interface PackageCheck {
  /** Modules reachable from the entry, each after the modules it imports. */
  order: string[];
  /** Per module, the resolved imports (null: the API). */
  links: Record<string, { ref: ImportRef; path: string | null }[]>;
  problems: string[];
}

/** Check every module reachable from `entry`: what it imports, what it contains, no cycles. */
export function checkPackage(files: Record<string, string>, entry: string): PackageCheck {
  const problems: string[] = [];
  const order: string[] = [];
  const links: PackageCheck["links"] = {};
  const state = new Map<string, "visiting" | "done">();
  const visit = (path: string, trail: string[]) => {
    const s = state.get(path);
    if (s === "done") return;
    if (s === "visiting") {
      problems.push(`modules import each other in a circle: ${[...trail.slice(trail.indexOf(path)), path].join(" -> ")}`);
      return;
    }
    state.set(path, "visiting");
    const scan = scanModule(path, files[path]);
    problems.push(...scan.problems);
    links[path] = [];
    for (const ref of scan.imports) {
      const r = resolveSpecifier(path, ref.spec, files);
      if ("problem" in r) problems.push(r.problem);
      else if ("api" in r) links[path].push({ ref, path: null });
      else {
        links[path].push({ ref, path: r.path });
        visit(r.path, [...trail, path]);
      }
    }
    state.set(path, "done");
    order.push(path);
  };
  if (!(entry in files)) problems.push(`the entry ${entry} is missing from the plugin`);
  else visit(entry, []);
  return { order, links, problems: [...new Set(problems)] };
}
