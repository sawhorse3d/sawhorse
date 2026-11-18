// The Help menu (lane 67, #97): what it holds in each build, and how its links open.
//
// The browser builds (the demo on the public site, `npm run dev` and `npm run preview`) open
// each page in a new tab. The desktop app opens it in the system's browser through the shell's
// open_external command (src-tauri/src/lib.rs), which opens only the pages it knows: Stripe
// Checkout and exactly the three pages below (HELP_URLS in src-tauri/device-key/src/lib.rs, a
// list src/lib/help.test.ts checks against this one). The app's own window never navigates.
import { openExternal } from "../credits/client";
import { isWebDemo, RELEASES_URL, REPO_URL, SITE_URL, sitePage } from "./site";

/** The public site's pages the desktop app opens, whatever base the build was made with. */
export const GUIDE_URL = `${SITE_URL}guide/`;
export const CHANGELOG_URL = `${SITE_URL}changelog/`;

/** One entry of the Help menu ("-" is a separator). */
export type HelpLink = "-" | { label: string; url: string; title?: string };

/**
 * The Help menu of a build. In the browser: User Guide, What's New, Download the Desktop App and
 * Source on GitHub; in the demo the site's pages are found relative to where it is served, so
 * they work on the github.io address too, and in every other browser build (`npm run dev`,
 * whose base is "/", and `npm run preview`) they are the public site's. In the desktop app:
 * User Guide, What's New and Source on GitHub, on the public site and GitHub.
 */
export function helpLinks(desktop: boolean, demo = isWebDemo()): HelpLink[] {
  if (desktop)
    return [
      { label: "User Guide", url: GUIDE_URL },
      { label: "What's New", url: CHANGELOG_URL },
      "-",
      { label: "Source on GitHub", url: REPO_URL },
    ];
  return [
    { label: "User Guide", url: demo ? sitePage("guide/") : GUIDE_URL },
    { label: "What's New", url: demo ? sitePage("changelog/") : CHANGELOG_URL },
    "-",
    { label: "Download the Desktop App", title: "Windows, macOS and Linux installers on GitHub", url: RELEASES_URL },
    { label: "Source on GitHub", url: REPO_URL },
  ];
}

/**
 * Open a Help page: a new tab in the browser, the system's browser in the desktop app. A shell
 * that refuses or fails is reported through `failed` with the address, so it can be typed.
 */
export function openLink(url: string, failed: (text: string) => void): void {
  openExternal(url).catch((err: unknown) => failed(`Could not open ${url} in the browser: ${err instanceof Error ? err.message : String(err)}`));
}
