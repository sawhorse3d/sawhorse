// What a plugin's code may contain, checked before it runs (by the app's loader, again by
// the sandbox as it links the modules, and by `npm run plugin:check`):
//
// - ES modules only. A module imports "sawhorse" (the API) or another module of the same
//   plugin by a relative path ("./lib/shelves.js"). Nothing else: no URLs, no packages, no
//   files outside the plugin folder.
// - No dynamic `import(...)` and no `importScripts`: everything a plugin runs is in its own
//   folder and was checked here. (The sandbox also removes importScripts and every way to
//   turn a string into code, so this is the first of two locks, not the only one.)
// - Modules may not import each other in a circle (each module becomes a blob URL that
//   must exist before the modules importing it).

export interface ImportRef {
  /** The specifier as written. */
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

const isIdent = (c: string) => /[A-Za-z0-9_$]/.test(c);

/**
 * The source with comments blanked out (same length, so positions still match), and with
 * `strings` also the text inside strings, templates and regular expressions (the quotes
 * stay). Regular-expression literals are recognised well enough that a quote or a slash
 * inside one does not start a string or a comment.
 */
export function maskComments(src: string, strings = false): string {
  const out = src.split("");
  let i = 0;
  let lastSig = "";
  let lastWord = "";
  const blank = (a: number, b: number) => {
    for (let k = a; k < b && k < out.length; k++) if (out[k] !== "\n") out[k] = " ";
  };
  const templates: number[] = [];
  /** Template text from `i` to its end or its next `${`. */
  const templateText = () => {
    const from = i;
    while (i < src.length && src[i] !== "`") {
      if (src[i] === "\\") i += 2;
      else if (src[i] === "$" && src[i + 1] === "{") {
        if (strings) blank(from, i);
        templates.push(0);
        i += 2;
        return;
      } else i++;
    }
    if (strings) blank(from, i);
    if (src[i] === "`") i++;
  };
  while (i < src.length) {
    const c = src[i];
    const d = src[i + 1];
    if (c === "/" && d === "/") {
      const e = src.indexOf("\n", i);
      const end = e < 0 ? src.length : e;
      blank(i, end);
      i = end;
      continue;
    }
    if (c === "/" && d === "*") {
      const e = src.indexOf("*/", i + 2);
      const end = e < 0 ? src.length : e + 2;
      blank(i, end);
      i = end;
      continue;
    }
    if (c === '"' || c === "'") {
      i++;
      const from = i;
      while (i < src.length && src[i] !== c && src[i] !== "\n") i += src[i] === "\\" ? 2 : 1;
      if (strings) blank(from, i);
      i++;
      lastSig = c;
      lastWord = "";
      continue;
    }
    if (c === "`") {
      i++;
      templateText();
      lastSig = "`";
      lastWord = "";
      continue;
    }
    if (c === "}" && templates.length && templates[templates.length - 1] === 0) {
      // Back into the template's text.
      templates.pop();
      i++;
      templateText();
      continue;
    }
    if (c === "{" && templates.length) templates[templates.length - 1]++;
    if (c === "}" && templates.length) templates[templates.length - 1]--;
    if (c === "/") {
      const regexAllowed = lastSig === "" || "(,=:[!&|?{};+-*%<>~^".includes(lastSig) || ["return", "typeof", "case", "do", "else", "in", "of", "void", "yield", "await"].includes(lastWord);
      if (regexAllowed) {
        i++;
        const from = i;
        let inClass = false;
        while (i < src.length && src[i] !== "\n") {
          if (src[i] === "\\") i += 2;
          else if (src[i] === "[") (inClass = true), i++;
          else if (src[i] === "]") (inClass = false), i++;
          else if (src[i] === "/" && !inClass) break;
          else i++;
        }
        if (strings) blank(from, i);
        i++;
        while (i < src.length && isIdent(src[i])) i++;
        lastSig = "/";
        lastWord = "";
        continue;
      }
    }
    if (isIdent(c)) {
      let j = i;
      while (j < src.length && isIdent(src[j])) j++;
      lastWord = src.slice(i, j);
      lastSig = "a";
      i = j;
      continue;
    }
    if (!/\s/.test(c)) {
      lastSig = c;
      lastWord = "";
    }
    i++;
  }
  return out.join("");
}

// Run on the source with string contents blanked (text inside strings never counts); the
// specifier is then read from the original at the same positions.
const STATIC_IMPORT = /\bimport\s*(?:[\w$*{}\s,]+?\s*\bfrom\s*)?(["'])( +)\1/g;
const STATIC_EXPORT = /\bexport\s*(?:\*(?:\s*as\s+[\w$]+)?|\{[^}]*\})\s*from\s*(["'])( +)\1/g;

function lineOf(src: string, at: number): number {
  let n = 1;
  for (let k = 0; k < at && k < src.length; k++) if (src[k] === "\n") n++;
  return n;
}

/** The imports of one module and anything in it that is not allowed. */
export function scanModule(path: string, src: string): ModuleScan {
  const code = maskComments(src, true);
  const problems: string[] = [];
  const imports: ImportRef[] = [];
  for (const m of code.matchAll(/\bimport\s*\(/g)) problems.push(`${path}:${lineOf(src, m.index ?? 0)}: dynamic import() is not allowed in a plugin (import modules statically from the plugin's own folder)`);
  for (const m of code.matchAll(/\bimportScripts\b/g)) problems.push(`${path}:${lineOf(src, m.index ?? 0)}: importScripts is not available to plugins`);
  for (const re of [STATIC_IMPORT, STATIC_EXPORT]) {
    re.lastIndex = 0;
    for (const m of code.matchAll(re)) {
      const whole = m.index ?? 0;
      const start = whole + m[0].length - 1 - m[2].length;
      const spec = src.slice(start, start + m[2].length);
      if (!spec.trim()) continue;
      imports.push({ spec, start, end: start + spec.length });
    }
  }
  imports.sort((a, b) => a.start - b.start);
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
