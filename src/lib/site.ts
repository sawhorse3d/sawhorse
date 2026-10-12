// The public site and the browser demo (lane 14).
//
// The same front end is served three ways: the Tauri desktop app, `npm run dev` /
// `npm run preview`, and the zero-install demo at https://sawhorse3d.com/app/ built by
// .github/workflows/pages.yml with VITE_BASE=/app/ and VITE_WEB_DEMO=1. Only that last
// build shows the demo banner and the Help menu's links back to the site.
import { isTauri } from "../io/files";

export const REPO_URL = "https://github.com/sawhorse3d/sawhorse";
/** The newest release (GitHub falls back to the release list while only pre-releases exist). */
export const RELEASES_URL = `${REPO_URL}/releases/latest`;
export const SITE_URL = "https://sawhorse3d.com/";

/** True in the demo build served from the public site (never in the desktop app). */
export function isWebDemo(): boolean {
  return import.meta.env.VITE_WEB_DEMO === "1" && !isTauri();
}

/**
 * A page of the public site, relative to where the demo is served (`/app/` -> `/guide/`),
 * so the links work on the github.io address and on the custom domain alike.
 */
export function sitePage(path: string): string {
  const base = import.meta.env.BASE_URL || "/";
  // BASE_URL is "/app/" in the demo build; "./" elsewhere, where the site is not beside us.
  if (!base.startsWith("/")) return new URL(path, SITE_URL).href;
  const parent = base.replace(/[^/]+\/$/, "");
  return `${parent}${path.replace(/^\//, "")}`;
}
