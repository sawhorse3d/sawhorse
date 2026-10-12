import { describe, expect, it } from "vitest";
import { checkPackage, normalisePath, parseModule, resolveSpecifier, scanModule } from "../src/source.ts";

describe("scanModule", () => {
  it("finds static imports and re-exports with their positions", () => {
    const src = `import api, { model } from "sawhorse";\nimport "./side.js";\nexport { a } from './lib/a.js';\nexport * from "./b.js";\nexport * as c from "./c.js";\nconst s = "import x from 'nope'"; const t = \`importScripts \${"import('y')"}\`; const k = self["import" + "Scripts"];`;
    const r = scanModule("main.js", src);
    expect(r.problems).toEqual([]);
    // Text inside strings never counts as an import or a forbidden word.
    expect(r.imports.map((i) => i.spec)).toEqual(["sawhorse", "./side.js", "./lib/a.js", "./b.js", "./c.js"]);
    for (const i of r.imports) expect(src.slice(i.start, i.end)).toBe(i.spec);
  });

  it("reads a specifier the way the module loader does (escapes resolved)", () => {
    const src = 'import x from "\\u002e/lib/a.js";';
    const r = scanModule("main.js", src);
    expect(r.problems).toEqual([]);
    expect(r.imports).toHaveLength(1);
    expect(r.imports[0].spec).toBe("./lib/a.js");
    // The text the sandbox replaces is everything inside the quotes.
    expect(src.slice(r.imports[0].start, r.imports[0].end)).toBe("\\u002e/lib/a.js");
    const hidden = scanModule("main.js", 'import x from "\\u0068ttps://example.com/x.js";');
    expect(hidden.imports[0].spec).toBe("https://example.com/x.js");
    expect(resolveSpecifier("main.js", hidden.imports[0].spec, {})).toHaveProperty("problem");
  });

  it("refuses dynamic import and importScripts, even behind a comment", () => {
    const r = scanModule("m.js", `const a = await import("https://x");\nimport/* sneaky */("./y.js");\nimportScripts("z.js");\nconst meta = import.meta.url;\nself.importScripts("w.js");`);
    expect(r.problems).toHaveLength(4);
    expect(r.problems[0]).toContain("m.js:1: dynamic import()");
    expect(r.problems[1]).toContain("m.js:2: dynamic import()");
    expect(r.problems[2]).toContain("m.js:3: importScripts");
    expect(r.problems[3]).toContain("m.js:5: importScripts");
  });

  it("ignores what is only in comments", () => {
    const r = scanModule("m.js", `// import("x")\n/* importScripts() */\nexport const n = 1;`);
    expect(r.problems).toEqual([]);
    expect(r.imports).toEqual([]);
  });

  // The verifier's bypasses of the old text scanner (it guessed regex versus division from
  // the previous token): a regex holding a quote or a backtick, placed where the guess was
  // "division", opened a phantom string that swallowed the import(...) after it. Each must
  // be found by the parser.
  const PAYLOAD = 'import("data:text/javascript,export default 1")';
  const bypasses: [string, string][] = [
    ["a regex after if (...)", `if (true) /"/.test("x"); ${PAYLOAD};`],
    ["a regex after while (...)", `let i = 0; while (i++ < 1) /'/.test("x"); ${PAYLOAD};`],
    ["a regex after throw", `function f() { throw /"/; }\n${PAYLOAD};`],
    ["a regex after try/throw", `try { throw /"/; } catch {}\n${PAYLOAD};`],
    ["a regex after delete", `delete /"/.x; ${PAYLOAD};`],
    ["a regex after export default", `export default /"/;\n${PAYLOAD};`],
    ["a backtick regex after )", `if (1) /\`/.test(""); ${PAYLOAD};`],
    ["a regex after a block", `{}\n/"/.test("x"); ${PAYLOAD};`],
    ["a regex after =>", `const f = () => /"/; ${PAYLOAD};`],
    ["a division that looks like a regex", `const a = 4, g = 2; const x = a /"/" + g; ${PAYLOAD};`],
    ["inside a template expression", `const t = \`\${/"/.source}\${${PAYLOAD}}\`;`],
    ["a phase import", `import.source("./x.js");`],
    ["an http URL", `${PAYLOAD.replace("data:text/javascript,export default 1", "http://localhost:5212/@vite/env.js?exfil=1")};`],
  ];
  for (const [what, src] of bypasses) {
    it(`finds import() hidden by ${what}`, () => {
      const r = checkPackage({ "main.js": src }, "main.js");
      expect(r.problems.length, src).toBeGreaterThan(0);
      expect(r.problems.join("\n"), src).toMatch(/dynamic import\(\) is not allowed|import\.source is not allowed|cannot be read as a JavaScript module/);
    });
  }

  it("the bypasses that are valid modules are refused for the import(), not for a parse error", () => {
    for (const [, src] of bypasses.slice(0, 11)) {
      expect(() => parseModule(src), src).not.toThrow();
      expect(scanModule("main.js", src).problems.join("\n"), src).toContain("dynamic import() is not allowed");
    }
  });

  it("refuses a module it cannot read, with the line", () => {
    const r = scanModule("lib/x.js", "export const a = 1;\nconst = ;");
    expect(r.problems).toHaveLength(1);
    expect(r.problems[0]).toMatch(/^lib\/x\.js:2: cannot be read as a JavaScript module: /);
    // HTML-like comments are script syntax, not module syntax: a module may not use them to
    // hide code (a browser refuses the module too).
    expect(scanModule("m.js", "const x = 1 <!-- import('y')\n").problems.join()).toContain("cannot be read");
  });

  it("allows import.meta, top-level await and a hashbang", () => {
    const r = scanModule("m.js", "#!/usr/bin/env node\nconst u = import.meta.url;\nawait Promise.resolve(u);\nexport {};");
    expect(r.problems).toEqual([]);
  });
});

describe("resolving", () => {
  const files = { "main.js": "", "lib/a.js": "", "lib/b.js": "" };
  it("normalises paths and refuses ones that leave the folder", () => {
    expect(normalisePath("lib/./x/../a.js")).toBe("lib/a.js");
    expect(normalisePath("../a.js")).toBeNull();
  });
  it("takes sawhorse and the plugin's own files only", () => {
    expect(resolveSpecifier("main.js", "sawhorse", files)).toEqual({ api: true });
    expect(resolveSpecifier("lib/a.js", "./b.js", files)).toEqual({ path: "lib/b.js" });
    expect(resolveSpecifier("lib/a.js", "../main.js", files)).toEqual({ path: "main.js" });
    expect(resolveSpecifier("main.js", "https://cdn.example.com/x.js", files)).toHaveProperty("problem", expect.stringContaining('may import only "sawhorse"'));
    expect(resolveSpecifier("main.js", "lodash", files)).toHaveProperty("problem");
    expect(resolveSpecifier("main.js", "../outside.js", files)).toHaveProperty("problem", expect.stringContaining("outside the plugin folder"));
    expect(resolveSpecifier("main.js", "./lib/a", files)).toHaveProperty("problem", expect.stringContaining(".js extension"));
    expect(resolveSpecifier("main.js", "./lib/c.js", files)).toHaveProperty("problem", expect.stringContaining("has no lib/c.js"));
  });
});

describe("checkPackage", () => {
  it("orders modules so each comes after what it imports", () => {
    const r = checkPackage({ "main.js": 'import "./a.js"; import "./b.js"; import "sawhorse";', "a.js": 'import "./b.js";', "b.js": "export {}", "unused.js": "import('x')" }, "main.js");
    expect(r.problems).toEqual([]);
    expect(r.order).toEqual(["b.js", "a.js", "main.js"]);
    expect(r.links["main.js"].map((l) => l.path)).toEqual(["a.js", "b.js", null]);
  });
  it("refuses circles, missing entries and bad imports", () => {
    expect(checkPackage({ "main.js": 'import "./a.js";', "a.js": 'import "./main.js";' }, "main.js").problems.join()).toContain("in a circle: main.js -> a.js -> main.js");
    expect(checkPackage({}, "main.js").problems).toEqual(["the entry main.js is missing from the plugin"]);
    expect(checkPackage({ "main.js": 'import x from "react";' }, "main.js").problems.join()).toContain('imports "react"');
  });
});
