import { defineConfig, devices } from "@playwright/test";

// Local Windows runs use the installed Edge/Chrome (no browser download on a
// metered link): PW_CHANNEL=msedge npm run e2e. CI installs Playwright's Chromium.
const channel = process.env.PW_CHANNEL || undefined;
// PW_PORT lets parallel worktrees run their suites side by side without one reusing the
// other's preview server (reuseExistingServer is on locally).
const port = Number(process.env.PW_PORT || 4173);

// Lane 16b: the specs that time the window (frame gaps, refusals) run after every other spec,
// one at a time, so no other browser shares the machine while they measure it. On a
// four-CPU CI runner, model-cap's Materials pane measured 15.7 s once while the spec beside
// it built and deflated a 90 MB scene.json; its calibration, taken a moment before, could
// not know. See docs/DEVELOPMENT.md, "Responsiveness bounds in the end-to-end tests".
const TIMED = /model-(cap|counts)\.spec\.ts$/;

export default defineConfig({
  testDir: "e2e",
  timeout: 30_000,
  retries: process.env.CI ? 1 : 0,
  use: {
    baseURL: `http://localhost:${port}`,
    trace: "retain-on-failure",
  },
  projects: [
    { name: "chromium", testIgnore: TIMED, use: { ...devices["Desktop Chrome"], channel, viewport: { width: 1440, height: 900 } } },
    { name: "chromium-timed", testMatch: TIMED, dependencies: ["chromium"], workers: 1, use: { ...devices["Desktop Chrome"], channel, viewport: { width: 1440, height: 900 } } },
  ],
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
