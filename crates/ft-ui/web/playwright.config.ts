import { defineConfig, devices } from '@playwright/test'

/**
 * Playwright config for the ft-ui happy-path e2e.
 *
 * The suite is opt-in: `pnpm test` (vitest) does NOT run it, only
 * `pnpm test:e2e`. We spawn the bundled-ui binary (built by `just ui-build`)
 * pointing at a tmp workspace, so the e2e exercises the same artifact users
 * ship — not the Vite dev server.
 *
 * Environment overrides:
 *   FIRETRAIL_E2E_BASE_URL — point at a running server instead of spawning one
 *                            (skips `webServer` entirely if set externally;
 *                            see the conditional below).
 *   FIRETRAIL_E2E_WORKSPACE — the workspace path to use; defaults to a tmp dir.
 */
const baseURL = process.env.FIRETRAIL_E2E_BASE_URL ?? 'http://127.0.0.1:5174'

export default defineConfig({
  testDir: './tests/e2e',
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  workers: 1,
  reporter: 'list',
  use: {
    baseURL,
    trace: 'on-first-retry',
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
  ],
  webServer: process.env.FIRETRAIL_E2E_BASE_URL
    ? undefined
    : {
        // We spawn the release binary built by `just ui-build`. The binary
        // prints the bound URL to stdout; Playwright waits for the port.
        command:
          'cargo run --manifest-path ../../../Cargo.toml -p ft-ui --features bundled-ui --release -- --workspace ' +
          `${process.env.FIRETRAIL_E2E_WORKSPACE ?? '/tmp/firetrail-e2e-ws'} --bind 127.0.0.1:5174 --foreground`,
        url: baseURL,
        reuseExistingServer: !process.env.CI,
        timeout: 120_000,
        stdout: 'pipe',
        stderr: 'pipe',
      },
})
