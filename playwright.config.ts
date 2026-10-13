import { defineConfig, devices } from "@playwright/test";

// Local Windows runs use the installed Edge/Chrome (no browser download on a
// metered link): PW_CHANNEL=msedge npm run e2e. CI installs Playwright's Chromium.
const channel = process.env.PW_CHANNEL || undefined;
// PW_PORT lets parallel worktrees run their suites side by side without one reusing the
// other's preview server (reuseExistingServer is on locally).
const port = Number(process.env.PW_PORT || 4173);

export default defineConfig({
  testDir: "e2e",
  timeout: 30_000,
  retries: process.env.CI ? 1 : 0,
  use: {
    baseURL: `http://localhost:${port}`,
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"], channel, viewport: { width: 1440, height: 900 } } }],
  // Lane 17: the specs run against their own build, the production build with the plugin
  // test hooks compiled in (dist-e2e/, scripts/e2e-serve.mjs); a release (npm run build, dist/)
  // has none. The build takes a minute or two, hence the timeout.
  webServer: {
    command: `node scripts/e2e-serve.mjs --port ${port}`,
    url: `http://localhost:${port}`,
    reuseExistingServer: !process.env.CI,
    timeout: 300_000,
  },
});
