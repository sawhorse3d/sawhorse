import { defineConfig, devices } from "@playwright/test";

// Local Windows runs use the installed Edge/Chrome (no browser download on a
// metered link): PW_CHANNEL=msedge npm run e2e. CI installs Playwright's Chromium.
const channel = process.env.PW_CHANNEL || undefined;
// PW_PORT lets parallel worktrees run e2e side by side (each lane its own preview port).
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
  webServer: {
    command: `npm run preview -- --port ${port} --strictPort`,
    url: `http://localhost:${port}`,
    reuseExistingServer: !process.env.CI,
  },
});
