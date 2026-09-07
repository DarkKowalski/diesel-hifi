import { defineConfig, devices } from '@playwright/test';

/**
 * End-to-end coverage runs against the *built* static site, served by
 * `vite preview`, so what the tests exercise is exactly what `web/dist` ships.
 *
 * Two projects cover SPEC section 10's "root and configurable-subpath previews":
 * the default relative-base build served at `/`, and the same source rebuilt
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
      // Relative-base build: one artifact that works at a domain root.
      command: 'vite build && vite preview --port 4173 --strictPort',
      url: `http://localhost:${ROOT_PORT}/`,
      reuseExistingServer: false,
      timeout: 180_000,
    },
    {
      // Absolute-base build served from a subpath.
      //
      // The base goes through `env` rather than through a `VAR=value command`
      // prefix. Playwright spawns the server through the platform shell, and on
      // Windows that is `cmd.exe`, which reads the prefix as a program name and
      // fails before Vite is reached — so the whole browser suite was
      // unrunnable there. `env` is passed to the process directly and needs no
      // shell syntax at all.
      command:
        'vite build --outDir dist-subpath && ' +
        'vite preview --outDir dist-subpath --port 4174 --strictPort',
      env: { VITE_BASE: SUBPATH },
      url: `http://localhost:${SUBPATH_PORT}${SUBPATH}`,
      reuseExistingServer: false,
      timeout: 180_000,
    },
  ],
});
