import { defineConfig, devices } from '@playwright/test';

/**
 * End-to-end coverage runs against the *built* static site, served by
 * `astro preview`, so what the tests exercise is exactly what `web/dist` ships.
 *
 * Two projects cover SPEC section 10's "root and configurable-subpath previews":
 * the root build served at `/`, and the same source rebuilt
 * with an absolute `VITE_BASE` and served under that subpath.
 */

const ROOT_PORT = 4173;
const SUBPATH_PORT = 4174;
const SUBPATH = '/diesel-hifi/';

export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 60_000,
  expect: { timeout: 15_000 },
  reporter: process.env.CI ? 'line' : [['list']],

  projects: [
    {
      name: 'root',
      use: {
        ...devices['Desktop Chrome'],
        baseURL: `http://localhost:${ROOT_PORT}/`,
      },
    },
    {
      name: 'subpath',
      use: {
        ...devices['Desktop Chrome'],
        baseURL: `http://localhost:${SUBPATH_PORT}${SUBPATH}`,
      },
    },
  ],

  webServer: [
    {
      // Root build.
      command: 'node scripts/test-server.mjs dist 4173 /',
      url: `http://localhost:${ROOT_PORT}/`,
      reuseExistingServer: false,
      timeout: 180_000,
    },
    {
      command: 'node scripts/test-server.mjs dist-subpath 4174 /diesel-hifi/',
      url: `http://localhost:${SUBPATH_PORT}${SUBPATH}`,
      reuseExistingServer: false,
      timeout: 180_000,
    },
  ],
});
