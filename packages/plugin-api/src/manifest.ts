// The plugin manifest (plugin.json): what a plugin is, what it may do and what it adds to
// the app. One validator, used by the app before a plugin is listed or started and by
// `npm run plugin:check` (scripts/plugin-check.mjs) before one is packed. It mirrors
// `manifest.schema.json` (a test keeps the two in step).
//
// Pure TypeScript with erasable syntax only, so Node runs it directly (type stripping) and
// Vite bundles it into the app.

/** The API version this package describes. */
export const API_VERSION = 1;

export type Permission = "model.read" | "model.write" | "ui" | "storage" | "file" | "net";

export const PERMISSIONS: readonly Permission[] = ["model.read", "model.write", "ui", "storage", "file", "net"];

/** What each permission lets a plugin do, in the words the enable dialog shows. */
export const PERMISSION_TEXT: Record<Permission, { title: string; detail: string }> = {
  // Lane 17: model.select is under model.read (METHODS), and the selection steers the
  // person's next Delete or Move, so the dialog says so.
  "model.read": { title: "Read the model", detail: "solids, groups and components, faces, materials, the cut list and the selection, and change the selection; also what is under the pointer while its tools are in use" },
  "model.write": { title: "Change the model", detail: "every change is one named undo step, checked by the kernel like your own edits" },
  ui: { title: "Add to the window", detail: "panels in the side panel, commands in the Plugins menu, tools on the rail (they follow the pointer, but see nothing of the model without \"Read the model\"), messages and questions" },
  storage: { title: "Keep settings", detail: "a small store of its own, for this app and inside the documents you save" },
  file: { title: "Save and open files", detail: "only through the save and open dialogs, where you pick the file (a browser without a save dialog asks you before each download); never programs or scripts" },
  net: { title: "Use the network", detail: "only the sites listed here" },
};

export interface CommandContribution {
  id: string;
  title: string;
}

export interface PanelContribution {
  id: string;
  title: string;
}

export interface ToolContribution {
  id: string;
  title: string;
  /** Status-line hint while the tool is active. */
  hint?: string;
  /** The Value Control Box label while the tool is active (absent: the box is read-only). */
  vcb?: string;
  /** An SVG path (`d`, in a 24 x 24 box) for the rail button. */
  icon?: string;
}

export interface MenuContribution {
  /** A command declared under `commands`. */
  command: string;
  /** Group in the plugin's submenu of the Plugins menu (items of one group sit together). */
  group?: string;
}

export interface ShortcutContribution {
  /** A command (or `tool`) declared by this plugin. */
  command?: string;
  tool?: string;
  /** "Alt+Shift+C", "Ctrl+Alt+K": at least Ctrl, Alt or Meta, then one letter, digit or F1..F12. */
  key: string;
}

export interface Contributions {
  commands: CommandContribution[];
  panels: PanelContribution[];
  tools: ToolContribution[];
  menus: MenuContribution[];
  shortcuts: ShortcutContribution[];
}

export interface Manifest {
  id: string;
  name: string;
  version: string;
  api: 1;
  description: string;
  author: string;
  license: string;
  permissions: Permission[];
  /** What the `net` permission reaches: hosts over https on the default port
   * ("api.example.com", "*.example.org"), origins ("https://api.example.com:8443",
   * "http://localhost:3000") or "localhost" (any port). See `parseNetEntry`. */
  net: string[];
  contributes: Contributions;
  entry: string;
  homepage?: string;
}

export type ManifestResult = { ok: true; manifest: Manifest } | { ok: false; errors: string[] };

export const ID_PATTERN = /^[a-z][a-z0-9-]*(\.[a-z0-9][a-z0-9-]*)+$/;
export const CONTRIBUTION_ID = /^[a-z][a-z0-9-]{0,39}$/;
export const VERSION_PATTERN = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$/;
/** A host name: lower-case labels and a top-level domain of letters ("api.example.com"). */
export const HOST_PATTERN = /^([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$/;
/**
 * One `net` entry (lane 17): `[scheme://][*.]host[:port]` or `localhost`. The validator adds
 * what a pattern cannot say: a port only with a scheme, http only for localhost, no wildcard
 * on localhost or on a public suffix.
 */
export const NET_ENTRY_PATTERN = /^(?:(https?):\/\/)?(\*\.)?(localhost|(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63})(?::(\d{1,5}))?$/;

/**
 * Domains under which anyone can have a name of their own (a public suffix): a wildcard on
 * one (`*.vercel.app`, `*.github.io`, `*.co.uk`) would reach every tenant, so the validator
 * refuses it and asks for the plugin's own host (`my-app.vercel.app`). Every plain top-level
 * domain (`*.com`, `*.dev`, `*.uk`) is one too. This is a short list of the common hosting
 * platforms and country second levels, not the whole Public Suffix List
 * (https://publicsuffix.org/list/): to refuse another, add it here, lower case and without
 * the leading dot, with a case in packages/plugin-api/test/manifest.test.ts ("refuses
 * wildcards on public suffixes"). A host under a suffix (`my-app.vercel.app`, or
 * `*.my-app.vercel.app` for its own subdomains) is always fine.
 */
export const PUBLIC_SUFFIXES: ReadonlySet<string> = new Set([
  // Hosting platforms: every subdomain is someone else's site.
  "vercel.app", "now.sh", "github.io", "githubusercontent.com", "gitlab.io", "netlify.app", "netlify.com", "pages.dev", "workers.dev", "herokuapp.com", "herokussl.com",
  "web.app", "firebaseapp.com", "appspot.com", "run.app", "cloudfunctions.net", "azurewebsites.net", "azurestaticapps.net", "cloudapp.net", "blob.core.windows.net",
  "cloudfront.net", "amazonaws.com", "s3.amazonaws.com", "elasticbeanstalk.com", "onrender.com", "fly.dev", "railway.app", "up.railway.app", "deno.dev", "glitch.me",
  "repl.co", "replit.app", "replit.dev", "surge.sh", "ngrok.io", "ngrok-free.app", "ngrok.app", "trycloudflare.com", "blogspot.com", "wordpress.com", "neocities.org",
  "readthedocs.io", "bitbucket.io", "codeberg.page", "sourceforge.io", "stackblitz.io", "webcontainer.io", "csb.app", "pythonanywhere.com", "duckdns.org", "dyndns.org", "no-ip.org",
  // Country second levels: co.uk is a suffix as much as uk.
  "co.uk", "org.uk", "me.uk", "ltd.uk", "plc.uk", "net.uk", "ac.uk", "gov.uk", "nhs.uk", "sch.uk",
  "com.au", "net.au", "org.au", "edu.au", "gov.au", "asn.au", "id.au",
  "co.nz", "net.nz", "org.nz", "govt.nz", "ac.nz", "co.jp", "ne.jp", "or.jp", "ac.jp", "go.jp",
  "co.kr", "or.kr", "com.cn", "net.cn", "org.cn", "com.hk", "com.tw", "com.sg", "com.my", "co.in", "net.in", "org.in", "co.id", "co.th", "com.ph", "com.vn",
  "co.za", "org.za", "com.ng", "co.ke", "com.eg", "com.tr", "com.br", "net.br", "org.br", "com.mx", "com.ar", "com.co", "com.pe", "co.il", "org.il", "com.sa", "com.ua", "com.pl",
]);

/** Whether `domain` is a public suffix: on the list, or a plain top-level domain. */
export function isPublicSuffix(domain: string): boolean {
  const d = domain.toLowerCase().replace(/\.$/, "");
  return !d.includes(".") || PUBLIC_SUFFIXES.has(d);
}

/** One `net` entry, read: what it lets the plugin reach. */
export interface NetEntry {
  /** "https", "http" (localhost only) or "any" (plain "localhost": http or https). */
  scheme: "https" | "http" | "any";
  host: string;
  /** `*.host`: the host's subdomains (not the host itself). */
  wildcard: boolean;
  /** A port, the scheme's default ("default") or, for plain "localhost", "any". */
  port: number | "default" | "any";
}

/** Read one `net` entry, or say why it is not one. */
export function parseNetEntry(raw: string): NetEntry | string {
  const m = NET_ENTRY_PATTERN.exec(raw);
  if (!m) return `must be a host like "api.example.com" or "*.example.com", an origin like "https://api.example.com:8443" or "http://localhost:3000", or "localhost" (lower case; no path)`;
  const [, scheme, star, host, port] = m;
  const local = host === "localhost";
  if (star && local) return '"*.localhost" is not a host: list "localhost"';
  if (port !== undefined && scheme === undefined) return `a port needs the scheme too, like "https://${star ?? ""}${host}:${port}"`;
  if (scheme === "http" && !local) return `plugins reach the network over https only (http is for localhost): "https://${star ?? ""}${host}${port ? `:${port}` : ""}"`;
  const n = port === undefined ? undefined : Number(port);
  if (n !== undefined && (n < 1 || n > 65535)) return `port ${port} is not a port (1 to 65535)`;
  if (star && isPublicSuffix(host)) return `"*.${host}" would reach every site under ${host}, where anyone can have a name of their own (a public suffix): list your own host, like "my-app.${host}"`;
  if (local && scheme === undefined) return { scheme: "any", host, wildcard: false, port: "any" };
  const s = (scheme ?? "https") as "https" | "http";
  return { scheme: s, host, wildcard: !!star, port: n === undefined || n === (s === "https" ? 443 : 80) ? "default" : n };
}

/** What a `net` entry reaches, in the words the enable dialog shows. */
export function describeNetEntry(raw: string): string {
  const e = parseNetEntry(raw);
  if (typeof e === "string") return raw;
  if (e.port === "any") return "any port on localhost (http or https)";
  const origin = `${e.scheme}://${e.wildcard ? "*." : ""}${e.host}${e.port === "default" ? "" : `:${e.port}`}`;
  return e.wildcard ? `${origin} (any subdomain of ${e.host})` : origin;
}
export const KEY_PATTERN = /^((Ctrl|Alt|Shift|Meta)\+)+([A-Z0-9]|F([1-9]|1[0-2]))$/;
/** A relative path inside the plugin folder: no "..", no leading "/", no backslashes. */
export const ENTRY_PATTERN = /^(?!\/)(?!.*(^|\/)\.\.?(\/|$))[A-Za-z0-9_\-./]+\.m?js$/;
/** An SVG path: commands, numbers, separators. Nothing that could become markup. */
export const ICON_PATTERN = /^[MmLlHhVvCcSsQqTtAaZz0-9eE.,\-+\s]{1,2000}$/;

const TOP_KEYS = ["$schema", "id", "name", "version", "api", "description", "author", "license", "permissions", "net", "contributes", "entry", "homepage"];
const CONTRIBUTION_KEYS = ["commands", "panels", "tools", "menus", "shortcuts"];

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

function text(errors: string[], where: string, v: unknown, min: number, max: number): string {
  if (typeof v !== "string") {
    errors.push(`${where} must be a string`);
    return "";
  }
  const n = [...v].length;
  if (n < min || n > max) errors.push(min === max ? `${where} must be ${min} characters` : `${where} must be ${min} to ${max} characters (it is ${n})`);
  return v;
}

function unknownKeys(errors: string[], where: string, o: Record<string, unknown>, allowed: string[]): void {
  for (const k of Object.keys(o)) if (!allowed.includes(k)) errors.push(`${where} has an unknown field "${k}"`);
}

function list<T>(errors: string[], where: string, v: unknown, item: (x: unknown, at: string) => T | null, max = 100): T[] {
  if (v === undefined) return [];
  if (!Array.isArray(v)) {
    errors.push(`${where} must be a list`);
    return [];
  }
  if (v.length > max) errors.push(`${where} may hold at most ${max} entries`);
  const out: T[] = [];
  v.forEach((x, i) => {
    const r = item(x, `${where}[${i}]`);
    if (r !== null) out.push(r);
  });
  return out;
}

function contributionId(errors: string[], where: string, v: unknown): string {
  if (typeof v !== "string" || !CONTRIBUTION_ID.test(v)) {
    errors.push(`${where} must be lower-case letters, digits and dashes, starting with a letter (at most 40)`);
    return "";
  }
  return v;
}

function uniqueIds(errors: string[], where: string, items: { id: string }[]): void {
  const seen = new Set<string>();
  for (const it of items) {
    if (seen.has(it.id)) errors.push(`${where}: "${it.id}" is declared twice`);
    seen.add(it.id);
  }
}

/** Latin look-alikes from other scripts that NFKC does not fold (Cyrillic, Greek). */
const LOOKALIKE: Record<string, string> = { "\u0430": "a", "\u0435": "e", "\u043e": "o", "\u0440": "p", "\u0441": "c", "\u0455": "s", "\u04bb": "h", "\u0501": "d", "\u051d": "w", "\u0461": "w", "\u03bf": "o", "\u03b1": "a", "\u03b5": "e", "\u03c1": "p", "\u0391": "a", "\u0395": "e", "\u039f": "o", "\u0405": "s", "\u0410": "a", "\u0415": "e", "\u041e": "o", "\u0420": "p", "\u0421": "c", "\u0397": "h", "\u041d": "h", "\u0428": "w" };

/**
 * Lane 17: whether a plugin's name would pass for the app's own: "Sawhorse", or "Sawhorse"
 * followed by anything but a letter ("Sawhorse Updates", "Sawhorse: Save", "Sawhorse3D"),
 * after folding case, compatibility forms (full-width letters), invisible characters,
 * spacing and common look-alike letters. "Sawhorses for kids" is fine.
 */
export function reservedName(name: string): boolean {
  const folded = [...name.normalize("NFKC").replace(/[\p{Cf}\p{Mn}]/gu, "")]
    .map((ch) => LOOKALIKE[ch] ?? ch)
    .join("")
    .toLowerCase()
    .replace(/\s+/g, " ")
    .trim();
  return /^saw ?horse(?![a-z])/.test(folded);
}

/** Check a parsed plugin.json. Every problem is listed, in the order found. */
export function validateManifest(raw: unknown): ManifestResult {
  const errors: string[] = [];
  if (!isObject(raw)) return { ok: false, errors: ["plugin.json must be a JSON object"] };
  unknownKeys(errors, "plugin.json", raw, TOP_KEYS);

  const id = text(errors, "id", raw.id, 3, 100);
  if (id && !ID_PATTERN.test(id)) errors.push(`id "${id}" must be reverse-DNS: lower-case words separated by dots, like "org.example.cabinet-generator"`);
  const name = text(errors, "name", raw.name, 1, 64);
  if (name && reservedName(name)) errors.push(`name "${name}" is reserved: the app's dialogs and status line say "from <name>", so a plugin may not be called Sawhorse or "Sawhorse ..."`);
  const version = text(errors, "version", raw.version, 1, 64);
  if (version && !VERSION_PATTERN.test(version)) errors.push(`version "${version}" must be a semantic version like 1.0.0`);
  if (raw.api !== API_VERSION) errors.push(`api must be ${API_VERSION} (this app speaks plugin API ${API_VERSION}; the plugin says ${JSON.stringify(raw.api)})`);
  const description = text(errors, "description", raw.description, 0, 500);
  const author = text(errors, "author", raw.author, 1, 100);
  const license = text(errors, "license", raw.license, 1, 64);
  const entry = raw.entry === undefined ? "main.js" : text(errors, "entry", raw.entry, 1, 200);
  if (entry && !ENTRY_PATTERN.test(entry)) errors.push(`entry "${entry}" must be a .js or .mjs file inside the plugin folder`);
  let homepage: string | undefined;
  if (raw.homepage !== undefined) {
    homepage = text(errors, "homepage", raw.homepage, 1, 300);
    if (homepage && !/^https:\/\/[^\s]+$/.test(homepage)) errors.push("homepage must be an https:// address");
  }

  const permissions = list(errors, "permissions", raw.permissions ?? [], (x, at) => {
    if (typeof x !== "string" || !(PERMISSIONS as readonly string[]).includes(x)) {
      errors.push(`${at} must be one of ${PERMISSIONS.join(", ")} (got ${JSON.stringify(x)})`);
      return null;
    }
    return x as Permission;
  });
  if (raw.permissions === undefined) errors.push("permissions must be listed (an empty list is fine)");
  if (new Set(permissions).size !== permissions.length) errors.push("permissions lists one permission twice");

  const net = list(errors, "net", raw.net, (x, at) => {
    const e = typeof x === "string" ? parseNetEntry(x) : "must be a string";
    if (typeof e === "string") {
      errors.push(`${at} ${JSON.stringify(typeof x === "string" ? x.slice(0, 100) : x)}: ${e}`);
      return null;
    }
    return x as string;
  }, 20);
  if (new Set(net).size !== net.length) errors.push("net lists one entry twice");
  if (permissions.includes("net") && net.length === 0) errors.push('the "net" permission needs "net": a list of the hosts it reaches');
  if (!permissions.includes("net") && net.length > 0) errors.push('"net" lists hosts but "permissions" does not ask for "net"');

  const c = raw.contributes === undefined ? {} : raw.contributes;
  const contributes: Contributions = { commands: [], panels: [], tools: [], menus: [], shortcuts: [] };
  if (!isObject(c)) errors.push("contributes must be an object");
  else {
    unknownKeys(errors, "contributes", c, CONTRIBUTION_KEYS);
    contributes.commands = list(errors, "contributes.commands", c.commands, (x, at) => {
      if (!isObject(x)) return errors.push(`${at} must be an object`), null;
      unknownKeys(errors, at, x, ["id", "title"]);
      return { id: contributionId(errors, `${at}.id`, x.id), title: text(errors, `${at}.title`, x.title, 1, 64) };
    });
    contributes.panels = list(errors, "contributes.panels", c.panels, (x, at) => {
      if (!isObject(x)) return errors.push(`${at} must be an object`), null;
      unknownKeys(errors, at, x, ["id", "title"]);
      return { id: contributionId(errors, `${at}.id`, x.id), title: text(errors, `${at}.title`, x.title, 1, 64) };
    }, 10);
    contributes.tools = list(errors, "contributes.tools", c.tools, (x, at) => {
      if (!isObject(x)) return errors.push(`${at} must be an object`), null;
      unknownKeys(errors, at, x, ["id", "title", "hint", "vcb", "icon"]);
      const t: ToolContribution = { id: contributionId(errors, `${at}.id`, x.id), title: text(errors, `${at}.title`, x.title, 1, 32) };
      if (x.hint !== undefined) t.hint = text(errors, `${at}.hint`, x.hint, 1, 200);
      if (x.vcb !== undefined) t.vcb = text(errors, `${at}.vcb`, x.vcb, 1, 40);
      if (x.icon !== undefined) {
        t.icon = text(errors, `${at}.icon`, x.icon, 1, 2000);
        if (t.icon && !ICON_PATTERN.test(t.icon)) errors.push(`${at}.icon must be SVG path data (M, L, C, A, Z ... and numbers)`);
      }
      return t;
    }, 10);
    contributes.menus = list(errors, "contributes.menus", c.menus, (x, at) => {
      if (!isObject(x)) return errors.push(`${at} must be an object`), null;
      unknownKeys(errors, at, x, ["command", "group"]);
      const m: MenuContribution = { command: contributionId(errors, `${at}.command`, x.command) };
      if (x.group !== undefined) m.group = text(errors, `${at}.group`, x.group, 1, 32);
      return m;
    });
    contributes.shortcuts = list(errors, "contributes.shortcuts", c.shortcuts, (x, at) => {
      if (!isObject(x)) return errors.push(`${at} must be an object`), null;
      unknownKeys(errors, at, x, ["command", "tool", "key"]);
      const s: ShortcutContribution = { key: text(errors, `${at}.key`, x.key, 1, 40) };
      if (s.key && !KEY_PATTERN.test(s.key)) errors.push(`${at}.key "${s.key}" must be modifiers and one key, like "Alt+Shift+C" (Ctrl, Alt or Meta required)`);
      else if (s.key && !/(Ctrl|Alt|Meta)\+/.test(s.key)) errors.push(`${at}.key "${s.key}" needs Ctrl, Alt or Meta (single keys belong to the built-in tools)`);
      if ((x.command === undefined) === (x.tool === undefined)) errors.push(`${at} must name exactly one of "command" or "tool"`);
      if (x.command !== undefined) s.command = contributionId(errors, `${at}.command`, x.command);
      if (x.tool !== undefined) s.tool = contributionId(errors, `${at}.tool`, x.tool);
      return s;
    }, 20);
    uniqueIds(errors, "contributes.commands", contributes.commands);
    uniqueIds(errors, "contributes.panels", contributes.panels);
    uniqueIds(errors, "contributes.tools", contributes.tools);
    const commands = new Set(contributes.commands.map((x) => x.id));
    const tools = new Set(contributes.tools.map((x) => x.id));
    for (const m of contributes.menus) if (m.command && !commands.has(m.command)) errors.push(`contributes.menus names a command "${m.command}" that contributes.commands does not declare`);
    for (const s of contributes.shortcuts) {
      if (s.command && !commands.has(s.command)) errors.push(`contributes.shortcuts names a command "${s.command}" that contributes.commands does not declare`);
      if (s.tool && !tools.has(s.tool)) errors.push(`contributes.shortcuts names a tool "${s.tool}" that contributes.tools does not declare`);
    }
    const keys = contributes.shortcuts.map((s) => s.key);
    if (new Set(keys).size !== keys.length) errors.push("contributes.shortcuts binds one key twice");
    const any = contributes.commands.length + contributes.panels.length + contributes.tools.length + contributes.menus.length + contributes.shortcuts.length;
    if (any > 0 && !permissions.includes("ui")) errors.push('contributes adds commands, panels, tools, menus or shortcuts: "permissions" must include "ui"');
  }

  if (errors.length) return { ok: false, errors };
  const manifest: Manifest = { id, name, version, api: 1, description, author, license, permissions, net, contributes, entry };
  if (homepage) manifest.homepage = homepage;
  return { ok: true, manifest };
}

/** Parse and check plugin.json text. */
export function parseManifest(json: string): ManifestResult {
  let raw: unknown;
  try {
    raw = JSON.parse(json);
  } catch (e) {
    return { ok: false, errors: [`plugin.json is not valid JSON: ${e instanceof Error ? e.message : String(e)}`] };
  }
  return validateManifest(raw);
}

/**
 * Whether `url` is on the manifest's `net` list, by its whole origin (lane 17: the host
 * alone let "localhost" reach every port, and "api.example.com" any port of it). A host
 * entry is https on the default port; an origin entry is that scheme, host and port; plain
 * "localhost" is any port of it over http or https; `*.host` is the host's subdomains.
 */
export function originAllowed(net: string[], url: string | URL): boolean {
  let u: URL;
  try {
    u = new URL(String(url));
  } catch {
    return false;
  }
  const scheme = u.protocol === "https:" ? "https" : u.protocol === "http:" ? "http" : null;
  if (!scheme || u.username || u.password) return false;
  const h = u.hostname.toLowerCase().replace(/\.$/, "");
  const port = u.port === "" ? (scheme === "https" ? 443 : 80) : Number(u.port);
  return net.some((raw) => {
    const e = parseNetEntry(raw);
    if (typeof e === "string") return false;
    if (e.scheme !== "any" && e.scheme !== scheme) return false;
    if (e.wildcard ? !(h.endsWith(`.${e.host}`) && h.length > e.host.length + 1) : h !== e.host) return false;
    if (e.port === "any") return true;
    return port === (e.port === "default" ? (e.scheme === "http" ? 80 : 443) : e.port);
  });
}
