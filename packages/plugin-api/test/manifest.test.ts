import { describe, expect, it } from "vitest";
import schemaJson from "../manifest.schema.json";
import { hostAllowed, parseManifest, PERMISSIONS, validateManifest } from "../src/manifest.ts";

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
    expect(errorsOf({ ...good, permissions: ["ui", "net"], net: ["https://api.example.com/x"] }).join()).toContain("no scheme, port or path");
    expect(errorsOf({ ...good, permissions: ["ui", "net"], net: ["api.example.com", "*.example.org"] })).toEqual([]);
  });

  it("reports bad JSON", () => {
    const r = parseManifest("{ nope");
    expect(r.ok).toBe(false);
    expect(!r.ok && r.errors[0]).toContain("not valid JSON");
  });
});

describe("hostAllowed", () => {
  it("matches exact hosts and subdomain wildcards only", () => {
    const net = ["api.example.com", "*.example.org"];
    expect(hostAllowed(net, "api.example.com")).toBe(true);
    expect(hostAllowed(net, "API.EXAMPLE.COM.")).toBe(true);
    expect(hostAllowed(net, "evil.api.example.com")).toBe(false);
    expect(hostAllowed(net, "a.example.org")).toBe(true);
    expect(hostAllowed(net, "example.org")).toBe(false);
    expect(hostAllowed(net, "badexample.org")).toBe(false);
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
  });
});
