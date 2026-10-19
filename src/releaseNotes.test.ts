// The current version's release notes (docs/releases/v<version>.md) are pasted as the GitHub
// release body (docs/DEVELOPMENT.md, release checklist step 6). GitHub resolves a relative
// link in a release body against the repository root without a branch, so "../LIMITS.md"
// becomes a dead page (the published alpha.2 release has two). Every link in the notes must
// therefore be absolute, or an anchor within the page.
import { existsSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const version = (JSON.parse(readFileSync("package.json", "utf8")) as { version: string }).version;
const notesPath = `docs/releases/v${version}.md`;

/** Markdown link and image targets: [text](target) and ![alt](target), plus <a href>. */
function linkTargets(md: string): string[] {
  const out: string[] = [];
  for (const m of md.matchAll(/!?\[[^\]]*\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)/g)) out.push(m[1]);
  for (const m of md.matchAll(/<a\s[^>]*href="([^"]+)"/gi)) out.push(m[1]);
  return out;
}

const fine = (target: string) => /^(https?:|mailto:)/i.test(target) || target.startsWith("#");

describe("release notes", () => {
  it(`exist for the package version (${version})`, () => {
    expect(existsSync(notesPath), notesPath).toBe(true);
  });

  it("link only to absolute URLs, so they work as a GitHub release body", () => {
    const md = readFileSync(notesPath, "utf8");
    const targets = linkTargets(md);
    expect(targets.length).toBeGreaterThan(0);
    expect(targets.filter((t) => !fine(t))).toEqual([]);
  });

  it("the link check finds relative links (it would have caught alpha.2's)", () => {
    expect(linkTargets("[a](../LIMITS.md) and ![b](img/x.png) and [c](https://x.org/y) and [d](#here)").filter((t) => !fine(t))).toEqual(["../LIMITS.md", "img/x.png"]);
    const alpha2 = readFileSync("docs/releases/v0.1.0-alpha.2.md", "utf8");
    expect(linkTargets(alpha2).filter((t) => !fine(t))).toEqual(["../LIMITS.md", "../skp-compat.md"]);
  });
});
