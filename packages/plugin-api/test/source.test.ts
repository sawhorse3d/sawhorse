import { describe, expect, it } from "vitest";
import { checkPackage, maskComments, normalisePath, resolveSpecifier, scanModule } from "../src/source.ts";

describe("scanModule", () => {
  it("finds static imports and re-exports with their positions", () => {
    const src = `import api, { model } from "sawhorse";\nimport "./side.js";\nexport { a } from './lib/a.js';\nexport * from "./b.js";\nconst s = "import x from 'nope'";`;
    const r = scanModule("main.js", src);
    expect(r.problems).toEqual([]);
    expect(r.imports.map((i) => i.spec)).toEqual(["sawhorse", "./side.js", "./lib/a.js", "./b.js", "nope"]);
    for (const i of r.imports) expect(src.slice(i.start, i.end)).toBe(i.spec);
  });

  it("refuses dynamic import and importScripts, even behind a comment", () => {
    const r = scanModule("m.js", `const a = await import("https://x");\nimport/* sneaky */("./y.js");\nimportScripts("z.js");\nconst meta = import.meta.url;`);
    expect(r.problems).toHaveLength(3);
    expect(r.problems[0]).toContain("m.js:1: dynamic import()");
    expect(r.problems[1]).toContain("m.js:2: dynamic import()");
    expect(r.problems[2]).toContain("m.js:3: importScripts");
  });

  it("ignores what is only in comments", () => {
    const r = scanModule("m.js", `// import("x")\n/* importScripts() */\nexport const n = 1;`);
    expect(r.problems).toEqual([]);
    expect(r.imports).toEqual([]);
  });
});

describe("maskComments", () => {
  it("keeps strings, regexes and templates; blanks comments; keeps positions", () => {
    const src = 'const a = "//not a comment"; const r = /\\/\\*x/g; const t = `a${"}"}b//c`; // gone\n/* gone */ x';
    const m = maskComments(src);
    expect(m.length).toBe(src.length);
    expect(m).toContain('"//not a comment"');
    expect(m).toContain("/\\/\\*x/g");
    expect(m).toContain('`a${"}"}b//c`');
    expect(m).not.toContain("gone");
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
