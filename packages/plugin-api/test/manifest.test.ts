import { describe, expect, it } from "vitest";
import schemaJson from "../manifest.schema.json";
import { describeNetEntry, isPublicSuffix, originAllowed, parseManifest, PERMISSIONS, validateManifest } from "../src/manifest.ts";

const good = {
  id: "org.example.cabinet-generator",
  name: "Cabinet generator",
  version: "1.2.0",
  api: 1,
  description: "Frameless cabinets.",
  author: "Sawhorse",
  license: "MIT",
  permissions: ["model.read", "model.write", "ui"],
  contributes: {
    commands: [{ id: "generate", title: "Generate cabinet" }],
    panels: [{ id: "cabinet", title: "Cabinet" }],
    tools: [{ id: "pins", title: "Pins", vcb: "Count, pitch", icon: "M4 4 L20 20 Z" }],
    menus: [{ command: "generate" }],
    shortcuts: [{ command: "generate", key: "Alt+Shift+C" }],
  },
  entry: "main.js",
};

const errorsOf = (m: unknown) => {
  const r = validateManifest(m);
  return r.ok ? [] : r.errors;
};

describe("validateManifest", () => {
  it("accepts a complete manifest and fills defaults", () => {
    const r = validateManifest(good);
    expect(r.ok).toBe(true);
    if (!r.ok) return;
    expect(r.manifest.entry).toBe("main.js");
    expect(r.manifest.net).toEqual([]);
    const { entry: _e, contributes: _c, ...minimal } = good;
    const m = validateManifest({ ...minimal, permissions: [] });
    expect(m.ok && m.manifest.entry).toBe("main.js");
    expect(m.ok && m.manifest.contributes).toEqual({ commands: [], panels: [], tools: [], menus: [], shortcuts: [] });
  });

  it.each([
    [{ id: "Cabinet" }, 'must be reverse-DNS'],
    [{ id: "nodots" }, "must be reverse-DNS"],
    [{ version: "1.0" }, "semantic version"],
    [{ api: 2 }, "api must be 1"],
    [{ name: "" }, "name must be 1 to 64 characters"],
    [{ license: 5 }, "license must be a string"],
    [{ permissions: ["model.read", "root"] }, "permissions[1] must be one of"],
    [{ permissions: ["ui", "ui"] }, "lists one permission twice"],
    [{ permissions: undefined }, "permissions must be listed"],
    [{ entry: "../evil.js" }, "inside the plugin folder"],
    [{ entry: "/abs.js" }, "inside the plugin folder"],
    [{ entry: "main.ts" }, ".js or .mjs"],
    [{ extra: 1 }, 'unknown field "extra"'],
    [{ homepage: "http://x.org" }, "https://"],
  ])("refuses %o", (patch, message) => {
    const m: Record<string, unknown> = { ...good, ...patch };
    for (const [k, v] of Object.entries(patch)) if (v === undefined) delete m[k];
    expect(errorsOf(m).join("\n")).toContain(message);
  });

  it("checks contributions against each other and against the ui permission", () => {
    const c = (contributes: unknown, permissions = ["ui"]) => errorsOf({ ...good, permissions, contributes }).join("\n");
    expect(c({ menus: [{ command: "nope" }] })).toContain('names a command "nope"');
    expect(c({ commands: [{ id: "a", title: "A" }], shortcuts: [{ command: "a", key: "C" }] })).toContain("modifiers and one key");
    expect(c({ commands: [{ id: "a", title: "A" }], shortcuts: [{ command: "a", key: "Shift+C" }] })).toContain("needs Ctrl, Alt or Meta");
    expect(c({ commands: [{ id: "a", title: "A" }], shortcuts: [{ key: "Alt+A" }] })).toContain('exactly one of "command" or "tool"');
    expect(c({ commands: [{ id: "a", title: "A" }, { id: "a", title: "B" }] })).toContain('"a" is declared twice');
    expect(c({ tools: [{ id: "t", title: "T", icon: "<script>" }] })).toContain("SVG path data");
    expect(c({ commands: [{ id: "Bad Id", title: "A" }] })).toContain("lower-case letters");
    expect(c({ commands: [{ id: "a", title: "A" }] }, ["model.read"])).toContain('"permissions" must include "ui"');
  });

  it("ties net hosts to the net permission", () => {
    expect(errorsOf({ ...good, permissions: ["net"], contributes: {} }).join()).toContain("needs \"net\": a list of the hosts");
    expect(errorsOf({ ...good, net: ["api.example.com"] }).join()).toContain('does not ask for "net"');
    expect(errorsOf({ ...good, permissions: ["ui", "net"], net: ["https://api.example.com/x"] }).join()).toContain("no path");
    expect(errorsOf({ ...good, permissions: ["ui", "net"], net: ["api.example.com", "*.example.org", "https://api.example.com:8443", "http://localhost:3000", "localhost"] })).toEqual([]);
  });

  it("refuses net entries that are not one origin (lane 17, F3)", () => {
    const net = (entry: string) => errorsOf({ ...good, permissions: ["ui", "net"], net: [entry] }).join("\n");
    expect(net("api.example.com:8443")).toContain("a port needs the scheme too");
    expect(net("http://api.example.com")).toContain("https only");
    expect(net("*.localhost")).toContain("not a host");
    expect(net("https://api.example.com:99999")).toContain("not a port");
    expect(net("ftp://api.example.com")).toContain("must be a host");
    expect(net("API.example.com")).toContain("lower case");
    expect(errorsOf({ ...good, permissions: ["ui", "net"], net: ["a.example.com", "a.example.com"] }).join()).toContain("one entry twice");
  });

  it("refuses wildcards on public suffixes: every tenant of a host would be reachable (lane 17, F3)", () => {
    const net = (entry: string) => errorsOf({ ...good, permissions: ["ui", "net"], net: [entry] }).join("\n");
    for (const s of ["vercel.app", "github.io", "netlify.app", "pages.dev", "herokuapp.com", "co.uk", "com.au", "workers.dev", "web.app"]) {
      expect(net(`*.${s}`), s).toContain("public suffix");
      expect(net(`https://*.${s}:8443`), s).toContain("public suffix");
      // A host of one's own under it is fine, and so are its own subdomains.
      expect(net(`my-app.${s}`), s).toBe("");
      expect(net(`*.my-app.${s}`), s).toBe("");
    }
    // Every plain top-level domain is one too.
    for (const tld of ["com", "org", "dev", "app", "io", "uk"]) expect(isPublicSuffix(tld)).toBe(true);
    expect(isPublicSuffix("example.com")).toBe(false);
    expect(isPublicSuffix("CO.UK.")).toBe(true);
  });

  it("describes each entry as what it reaches", () => {
    expect(describeNetEntry("api.example.com")).toBe("https://api.example.com");
    expect(describeNetEntry("*.example.org")).toBe("https://*.example.org (any subdomain of example.org)");
    expect(describeNetEntry("https://api.example.com:8443")).toBe("https://api.example.com:8443");
    expect(describeNetEntry("https://api.example.com:443")).toBe("https://api.example.com");
    expect(describeNetEntry("http://localhost:3000")).toBe("http://localhost:3000");
    expect(describeNetEntry("localhost")).toBe("any port on localhost (http or https)");
  });

  it("reports bad JSON", () => {
    const r = parseManifest("{ nope");
    expect(r.ok).toBe(false);
    expect(!r.ok && r.errors[0]).toContain("not valid JSON");
  });
});

describe("originAllowed (lane 17, F3: the whole origin, not the host)", () => {
  it("matches exact hosts and subdomain wildcards only, over https on the default port", () => {
    const net = ["api.example.com", "*.example.org"];
    expect(originAllowed(net, "https://api.example.com/v1")).toBe(true);
    expect(originAllowed(net, "https://API.EXAMPLE.COM./v1")).toBe(true);
    expect(originAllowed(net, "https://api.example.com:443/")).toBe(true);
    expect(originAllowed(net, "https://evil.api.example.com/")).toBe(false);
    expect(originAllowed(net, "https://a.example.org/")).toBe(true);
    expect(originAllowed(net, "https://example.org/")).toBe(false);
    expect(originAllowed(net, "https://badexample.org/")).toBe(false);
    // The port and the scheme count.
    expect(originAllowed(net, "https://api.example.com:8443/")).toBe(false);
    expect(originAllowed(net, "https://a.example.org:8080/")).toBe(false);
    expect(originAllowed(net, "http://api.example.com/")).toBe(false);
    expect(originAllowed(net, "https://user:pw@api.example.com/")).toBe(false);
    expect(originAllowed(net, "not a url")).toBe(false);
  });

  it("an origin entry is that scheme, host and port; plain localhost is any port of it", () => {
    expect(originAllowed(["https://api.example.com:8443"], "https://api.example.com:8443/x")).toBe(true);
    expect(originAllowed(["https://api.example.com:8443"], "https://api.example.com/x")).toBe(false);
    expect(originAllowed(["http://localhost:3000"], "http://localhost:3000/x")).toBe(true);
    expect(originAllowed(["http://localhost:3000"], "http://localhost:3001/x")).toBe(false);
    expect(originAllowed(["http://localhost:3000"], "https://localhost:3000/x")).toBe(false);
    expect(originAllowed(["http://localhost"], "http://localhost/x")).toBe(true);
    expect(originAllowed(["http://localhost"], "http://localhost:8080/x")).toBe(false);
    expect(originAllowed(["localhost"], "http://localhost:5432/x")).toBe(true);
    expect(originAllowed(["localhost"], "https://localhost/x")).toBe(true);
    expect(originAllowed(["localhost"], "http://127.0.0.1:5432/x")).toBe(false);
    expect(originAllowed(["localhost"], "http://localhost.example.com/")).toBe(false);
  });
});

describe("manifest.schema.json", () => {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const schema = schemaJson as any;
  it("lists the same fields, permissions and patterns as the validator", () => {
    expect(Object.keys(schema.properties).sort()).toEqual(["$schema", "api", "author", "contributes", "description", "entry", "homepage", "id", "license", "name", "net", "permissions", "version"]);
    expect(schema.properties.permissions.items.enum).toEqual([...PERMISSIONS]);
    expect(Object.keys(schema.properties.contributes.properties).sort()).toEqual(["commands", "menus", "panels", "shortcuts", "tools"]);
    // The schema's patterns accept what the validator accepts.
    expect(new RegExp(schema.properties.id.pattern).test(good.id)).toBe(true);
    expect(new RegExp(schema.properties.version.pattern).test("1.2.0-beta.1")).toBe(true);
    expect(new RegExp(schema.properties.entry.pattern).test("lib/main.mjs")).toBe(true);
    expect(new RegExp(schema.properties.entry.pattern).test("../x.js")).toBe(false);
    // The net entries the validator takes, the schema takes; what the pattern alone can
    // refuse, it refuses.
    const net = new RegExp(schema.properties.net.items.pattern);
    for (const ok of ["api.example.com", "*.example.org", "https://api.example.com:8443", "https://*.example.org", "http://localhost:3000", "https://localhost", "localhost"]) {
      expect(net.test(ok), ok).toBe(true);
      expect(errorsOf({ ...good, permissions: ["ui", "net"], net: [ok] }), ok).toEqual([]);
    }
    for (const bad of ["http://api.example.com", "api.example.com:8443", "*.localhost", "https://api.example.com/x", "ftp://a.example.com"]) {
      expect(net.test(bad), bad).toBe(false);
      expect(errorsOf({ ...good, permissions: ["ui", "net"], net: [bad] }).length, bad).toBeGreaterThan(0);
    }
  });
});
