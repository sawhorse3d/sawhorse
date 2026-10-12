// The sample plugins shipped with the app (plugins/ in the repository). They are bundled as
// text and listed on start, disabled until the person enables them (the permissions are
// shown first), the same way in the browser and on the desktop.

const raw = import.meta.glob<string>("../../plugins/*/**/*", { query: "?raw", import: "default", eager: true });

/** Each bundled plugin's files, keyed by the plugin folder's name. */
export function bundledPlugins(): Record<string, Record<string, string>> {
  const out: Record<string, Record<string, string>> = {};
  for (const [key, text] of Object.entries(raw)) {
    const m = /\/plugins\/([^/]+)\/(.+)$/.exec(key);
    if (!m) continue;
    (out[m[1]] ??= {})[m[2]] = text;
  }
  return out;
}
