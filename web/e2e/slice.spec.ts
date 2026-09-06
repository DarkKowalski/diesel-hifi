import { expect, test, type Page, type Request } from '@playwright/test';

/**
 * End-to-end coverage of the vertical slice, run against the built static site.
 *
 * Covers SPEC section 10: WASM loads and advances in a browser through the
 * worker; the UI stays responsive while the simulation runs and takes its
 * selector entries from the real API; and the build makes no external requests.
 *
 * The suite runs twice — once at a domain root, once under a configured
 * subpath — via the two Playwright projects.
 */

const ENGINE_ID = 'mercedes-benz-om471-9-m3d-375kw';

/** Collects any request that leaves the page's own origin. */
function watchForExternalRequests(page: Page): string[] {
  const external: string[] = [];
  page.on('request', (request: Request) => {
    const url = new URL(request.url());
    if (url.protocol === 'blob:' || url.protocol === 'data:') return;
    if (url.hostname !== 'localhost' && url.hostname !== '127.0.0.1') {
      external.push(request.url());
    }
  });
  return external;
}

async function bootstrap(page: Page) {
  const external = watchForExternalRequests(page);
  await page.goto('./');
  await expect(page.getByTestId('lifecycle')).toHaveText('ready');
  return external;
}

/** Telemetry readings carry unit suffixes, so parse rather than cast. */
async function numeric(page: Page, testId: string): Promise<number> {
  return Number.parseFloat(await page.getByTestId(testId).innerText());
}

async function rpm(page: Page): Promise<number> {
  return numeric(page, 'rpm');
}

test('the worker boots and the selector is populated from the real catalog API', async ({
  page,
}) => {
  await bootstrap(page);

  // The selector renders only after the worker answers `listConfigs`.
  const select = page.getByTestId('engine-select');
  await expect(select).toHaveAttribute('data-source', 'catalog-api');
  await expect(select.locator('option')).toHaveCount(1);
  await expect(select).toHaveValue(ENGINE_ID);
  await expect(page.getByTestId('active-id')).toHaveText(ENGINE_ID);
  await expect(page.getByTestId('disclaimer')).not.toBeEmpty();

  // Versions come from the WASM module, so a boot means WASM really loaded.
  await expect(page.getByTestId('status-bar')).toContainText('api v2');
  await expect(page.getByTestId('status-bar')).toContainText('Web Worker');
});

test('provenance crosses the boundary and is visible in the UI', async ({ page }) => {
  await bootstrap(page);

  const published = Number(await page.getByTestId('count-published').innerText());
  const calibrated = Number(await page.getByTestId('count-calibrated').innerText());
  const derived = Number(await page.getByTestId('count-derived').innerText());

  expect(published).toBeGreaterThan(0);
  expect(calibrated).toBeGreaterThan(0);
  expect(published + calibrated + derived).toBeGreaterThan(40);

  // Calibration targets that are not published must be visible as such.
  const entries = page.getByTestId('provenance-entries');
  await expect(entries).toContainText('inertia.rotating_inertia_kg_m2');
  await expect(entries).toContainText('geometry.firing_order');
});

test('starting the engine advances the simulation through the worker', async ({ page }) => {
  await bootstrap(page);

  await expect(page.getByTestId('run-state')).toHaveText('stopped');
  await expect(page.getByTestId('rpm')).toHaveText('0');

  await page.getByTestId('start').click();

  // The engine cranks and then runs on its own.
  await expect(page.getByTestId('run-state')).toHaveText('running', { timeout: 20_000 });
  await expect
    .poll(() => rpm(page), { timeout: 20_000, message: 'the engine should reach idle' })
    .toBeGreaterThan(400);

  // Telemetry that can only come from the physics.
  await expect
    .poll(() => numeric(page, 'peak-session'), {
      message: 'compression should build real cylinder pressure',
    })
    .toBeGreaterThan(1);
  await expect(page.getByTestId('cylinders').locator('li')).toHaveCount(6);
  await expect.poll(() => numeric(page, 'steps')).toBeGreaterThan(10_000);
});

test('the UI stays responsive while the simulation runs', async ({ page }) => {
  await bootstrap(page);
  await page.getByTestId('start').click();
  await expect(page.getByTestId('run-state')).toHaveText('running', { timeout: 20_000 });

  const before = await rpm(page);

  // Interacting with the page while the worker is stepping must be immediate.
  const started = Date.now();
  await page.getByTestId('pedal').fill('80');
  await page.getByTestId('load').fill('500');
  const elapsed = Date.now() - started;
  expect(elapsed, 'input handling must not be blocked by the simulation').toBeLessThan(3_000);

  // Opening full pedal must change the engine speed, proving the input reached
  // the physics through the worker.
  await expect
    .poll(() => rpm(page), { timeout: 20_000, message: 'the pedal should raise engine speed' })
    .toBeGreaterThan(before + 100);
});

test('stopping the engine brings it to rest and reset clears telemetry', async ({ page }) => {
  await bootstrap(page);
  await page.getByTestId('start').click();
  await expect(page.getByTestId('run-state')).toHaveText('running', { timeout: 20_000 });

  await page.getByTestId('stop').click();
  await expect
    .poll(() => rpm(page), { timeout: 30_000, message: 'the engine should coast down' })
    .toBe(0);
  await expect(page.getByTestId('run-state')).toHaveText('stopped');

  await page.getByTestId('reset').click();
  await expect(page.getByTestId('sim-time')).toHaveText('0.00 s');
  await expect(page.getByTestId('steps')).toHaveText('0');
});

test('re-selecting the active engine performs a deterministic reset', async ({ page }) => {
  await bootstrap(page);
  await page.getByTestId('start').click();
  await expect
    .poll(() => numeric(page, 'steps'), { timeout: 20_000 })
    .toBeGreaterThan(1_000);

  // Selecting the configuration that is already active must reset the run.
  await page.getByTestId('engine-select').selectOption(ENGINE_ID);

  await expect(page.getByTestId('steps')).toHaveText('0');
  await expect(page.getByTestId('sim-time')).toHaveText('0.00 s');
  await expect(page.getByTestId('rpm')).toHaveText('0');
  await expect(page.getByTestId('run-state')).toHaveText('stopped');
});

test('the built site makes no external network requests', async ({ page }) => {
  const external = await bootstrap(page);

  await page.getByTestId('start').click();
  await expect(page.getByTestId('run-state')).toHaveText('running', { timeout: 20_000 });
  await page.waitForTimeout(1_500);

  expect(external, `unexpected external requests: ${external.join(', ')}`).toEqual([]);
});

test('cycle-averaged torque and power appear once the engine runs', async ({ page }) => {
  await bootstrap(page);
  await page.getByTestId('start').click();
  await expect(page.getByTestId('run-state')).toHaveText('running', { timeout: 20_000 });

  // Whole-cycle averages only exist after a complete four-stroke cycle.
  await expect(page.getByTestId('cycle-averages')).toBeVisible({ timeout: 20_000 });
  await expect.poll(() => numeric(page, 'brake-power')).not.toBeNaN();

  // Milestone 2 combustion telemetry: the published APCRS variant is reported.
  await expect(page.getByTestId('variant')).toHaveText(/standard|amplified/);

  // Load the engine so the boost schedule actually spools.
  await page.getByTestId('pedal').fill('90');
  await page.getByTestId('load').fill('1500');
  await expect
    .poll(() => numeric(page, 'intake-pressure'), {
      timeout: 25_000,
      message: 'charge pressure should rise above ambient under load',
    })
    .toBeGreaterThan(1.1);
});

test('the dynamometer sweep renders a calibrated curve', async ({ page }) => {
  await bootstrap(page);

  await expect(page.getByTestId('dyno-chart')).toHaveCount(0);
  await page.getByTestId('run-sweep').click();

  // The chart appears once every point is measured.
  await expect(page.getByTestId('dyno-chart')).toBeVisible({ timeout: 120_000 });

  // Peaks are labelled CALIBRATED, never presented as OEM data, and the
  // published magnitudes are drawn as a separate reference.
  await expect(page.getByTestId('peak-power-label')).toContainText('CALIBRATED');
  await expect(page.getByTestId('peak-torque-label')).toContainText('CALIBRATED');
  await expect(page.getByTestId('dyno-chart')).toContainText('PUBLISHED 375');
  await expect(page.getByTestId('dyno-chart')).toContainText('PUBLISHED 2500');
  await expect(page.getByTestId('calibration-note')).toContainText('not OEM data');

  // The measured peaks land near the published magnitudes.
  const peaks = page.getByTestId('dyno-peaks');
  await expect(peaks).toBeVisible();
  const text = await peaks.innerText();
  const power = Number.parseFloat(text.match(/([\d.]+) kW/)?.[1] ?? 'NaN');
  const torque = Number.parseFloat(text.match(/([\d]+) N·m/)?.[1] ?? 'NaN');
  expect(Math.abs(power - 375) / 375).toBeLessThanOrEqual(0.03);
  expect(Math.abs(torque - 2500) / 2500).toBeLessThanOrEqual(0.03);

  // An accessible table view of the same numbers exists.
  await page.getByRole('button', { name: 'Show table' }).click();
  await expect(page.getByTestId('sweep-table')).toBeVisible();
  expect(await page.getByTestId('sweep-table').locator('tbody tr').count()).toBeGreaterThan(9);
});
