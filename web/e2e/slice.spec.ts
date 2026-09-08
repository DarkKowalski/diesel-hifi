import { expect, test, type Page, type Request } from '@playwright/test';

/**
 * End-to-end coverage of the vertical slice, run against the built static site.
 *
 * Covers README "Acceptance criteria": WASM loads and advances in a browser through the
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

/**
 * Wait for the engine to reach governed idle.
 *
 * `run-state` reads `running` as soon as the worker is stepping, which is while
 * the starter is still dragging the crank up through a few hundred rpm — a long
 * way below the 560 rpm the governor holds. Loading it there is loading an
 * engine that has not caught yet.
 *
 * **This is a real margin rather than a theoretical one.** Reproduced natively,
 * applying 500 N·m at 395 rpm dips the crank to 269 rpm before the turbo has any
 * boost to give, and 269 rpm is close enough to a stall that how many steps the
 * worker got through in the last animation frame decides the outcome. Browser
 * step counts come from wall-clock time by design, so under load the dip goes
 * deeper. Six of these tests failed on the previous solver and five on the
 * current one, in a *different combination* each run and all with the same
 * symptom — an engine reading `stopped` where a speed was expected.
 *
 * Waiting for idle is what a driver does and it removes the coupling to machine
 * timing entirely.
 */
async function awaitIdle(page: Page): Promise<void> {
  await expect
    .poll(() => rpm(page), {
      timeout: 30_000,
      message: 'the engine should reach governed idle before it is loaded',
    })
    .toBeGreaterThan(500);
}

/**
 * Bring a running engine up to a loaded working point.
 *
 * The load is ramped rather than dropped on all at once. Boost is no longer
 * prescribed from a schedule — it has to be earned from exhaust energy — so an
 * idling engine buried under full load simply stalls, and a stalled engine makes
 * no boost. A real truck pulls away the same way, and it waits for the engine to
 * be idling first.
 */
async function pullAway(page: Page): Promise<void> {
  await awaitIdle(page);
  await page.getByTestId('pedal').fill('90');
  await page.getByTestId('load').fill('500');
  await expect
    .poll(() => rpm(page), { timeout: 30_000, message: 'the engine should pick up speed' })
    .toBeGreaterThan(1_100);
  await page.getByTestId('load').fill('1800');
}

/** Bar heights from the spectrum view, which is drawn off the output path. */
async function spectrumBars(page: Page): Promise<number[]> {
  return page
    .getByTestId('spectrum')
    .locator('rect.bar')
    .evaluateAll((nodes) =>
      nodes.map((node) => Number.parseFloat(node.getAttribute('height') ?? '0')),
    );
}

/**
 * Which bar a frequency lands in.
 *
 * The axis is logarithmic from 20 Hz to 20 kHz across all the bars, so the
 * bucket for a frequency f is floor(log10(f/20) / log10(1000) * count).
 */
function bucketOf(hz: number, buckets: number): number {
  return Math.floor((Math.log10(hz / 20) / Math.log10(20_000 / 20)) * buckets);
}

/** Mean bar height across a frequency band. */
function bandMean(bars: number[], lowHz: number, highHz: number): number {
  const slice = bars.slice(bucketOf(lowHz, bars.length), bucketOf(highHz, bars.length));
  return slice.reduce((sum, height) => sum + height, 0) / Math.max(1, slice.length);
}

/**
 * Average the output over a couple of seconds.
 *
 * The exhaust is a pulse train, not a tone: any single reading of either the
 * spectrum or the level is a snapshot of something that moves with every firing
 * event. Comparing two stages on single readings compares the moment they were
 * taken in.
 */
/** Bar heights averaged over a couple of seconds, bin by bin. */
async function averagedBars(page: Page): Promise<number[]> {
  const sums: number[] = [];
  const reads = 10;
  for (let i = 0; i < reads; i += 1) {
    await page.waitForTimeout(200);
    const bars = await spectrumBars(page);
    bars.forEach((height, bin) => {
      sums[bin] = (sums[bin] ?? 0) + height;
    });
  }
  return sums.map((sum) => sum / reads);
}

async function measureOutput(page: Page): Promise<{ low: number; high: number; levelDb: number }> {
  const lows: number[] = [];
  const highs: number[] = [];
  const powers: number[] = [];
  for (let i = 0; i < 10; i += 1) {
    await page.waitForTimeout(200);
    const bars = await spectrumBars(page);
    lows.push(bandMean(bars, 60, 400));
    highs.push(bandMean(bars, 2_000, 8_000));
    const db = Number.parseFloat(await page.getByTestId('audio-output-level').innerText());
    // Average power, not decibels: the mean of a set of logarithms is not the
    // logarithm of their mean, and it is loudness that is being compared.
    powers.push(10 ** (db / 10));
  }
  const mean = (values: number[]) => values.reduce((sum, v) => sum + v, 0) / values.length;
  return { low: mean(lows), high: mean(highs), levelDb: 10 * Math.log10(mean(powers)) };
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
  await expect(page.getByTestId('status-bar')).toContainText('api v4');
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

  // Idle first, for the reason `awaitIdle` gives: 500 N·m applied to an engine
  // still on the starter is a coin toss decided by how many steps the last frame
  // managed, and this test is about input latency rather than about that.
  await awaitIdle(page);
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

  // Load the engine so the turbocharger actually spools.
  await pullAway(page);
  await expect
    .poll(() => numeric(page, 'intake-pressure'), {
      timeout: 30_000,
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

test('the air path is computed rather than prescribed', async ({ page }) => {
  await bootstrap(page);
  await page.getByTestId('start').click();
  await expect(page.getByTestId('run-state')).toHaveText('running', { timeout: 20_000 });

  // Load it so the turbo has exhaust energy to work with.
  await pullAway(page);

  // Boost is the output of a compressor on a shaft with inertia, so the shaft
  // must actually be turning for boost to exist.
  await expect
    .poll(() => numeric(page, 'turbo-shaft'), {
      timeout: 30_000,
      message: 'the turbo shaft should spin up under load',
    })
    .toBeGreaterThan(10);
  await expect
    .poll(() => numeric(page, 'boost'), { timeout: 30_000 })
    .toBeGreaterThan(0.3);

  // Exhaust manifold pressure is a state too, not a fixed boundary condition.
  await expect.poll(() => numeric(page, 'exhaust-pressure')).toBeGreaterThan(1.1);
});

test('switching EGR off changes what reaches the cylinders', async ({ page }) => {
  await bootstrap(page);
  await page.getByTestId('start').click();
  await expect(page.getByTestId('run-state')).toHaveText('running', { timeout: 20_000 });

  await pullAway(page);

  const toggle = page.getByTestId('egr-toggle');
  await expect(toggle).toBeChecked();

  // Recirculation dilutes the intake charge, which is what the smoke limit sees.
  await expect
    .poll(() => numeric(page, 'intake-burned'), {
      timeout: 30_000,
      message: 'recirculated exhaust should show up in the intake',
    })
    .toBeGreaterThan(0.5);

  await toggle.uncheck();

  // With the valve shut the loop stops and the intake cleans up.
  await expect
    .poll(() => numeric(page, 'egr-valve'), { timeout: 20_000 })
    .toBe(0);
  await expect
    .poll(() => numeric(page, 'intake-burned'), { timeout: 30_000 })
    .toBeLessThan(0.5);
});

test('sound starts only from an explicit action and then really plays', async ({ page }) => {
  await bootstrap(page);

  // README "WASM and worker API": nothing audio-related exists before the user asks for
  // it.
  await expect(page.getByTestId('audio-status')).toHaveCount(0);
  await expect(
    page.evaluate(() => (window as unknown as { AudioContext?: unknown }).AudioContext !== undefined),
  ).resolves.toBe(true);

  await page.getByTestId('start').click();
  await expect(page.getByTestId('run-state')).toHaveText('running', { timeout: 20_000 });

  await page.getByTestId('enable-audio').click();
  await expect(page.getByTestId('audio-status')).toBeVisible({ timeout: 20_000 });

  // The solver produces at its own fixed-step rate; the worklet resamples.
  await expect(page.getByTestId('audio-source-rate')).toHaveText('40.0 kHz');
  await expect
    .poll(() => numeric(page, 'audio-device-rate'), { timeout: 10_000 })
    .toBeGreaterThan(0);

  // Samples must actually reach the worklet, which is what proves the whole
  // chain works: solver -> worker -> main thread -> AudioWorklet.
  await expect
    .poll(() => numeric(page, 'audio-received'), {
      timeout: 30_000,
      message: 'the worklet should be receiving exhaust samples',
    })
    .toBeGreaterThan(10_000);

  await page.getByTestId('disable-audio').click();
  await expect(page.getByTestId('enable-audio')).toBeVisible();
});

test('the exhaust output has energy where a speaker can reproduce it', async ({ page }) => {
  await bootstrap(page);
  await page.getByTestId('start').click();
  await expect(page.getByTestId('run-state')).toHaveText('running', { timeout: 20_000 });

  await page.getByTestId('enable-audio').click();
  await expect(page.getByTestId('spectrum')).toBeVisible({ timeout: 20_000 });

  // The raw stage on purpose. This asserts something about the signal the
  // *solver* produces, and measuring it through the cab filter would be
  // measuring the filter.
  await page.getByTestId('audio-stage-raw').click();

  await pullAway(page);
  await page.waitForTimeout(2_000);

  // Bars are drawn from the analyser on the *output* path, so a bar with height
  // is energy actually being played.
  //
  // This is the assertion that a "finite, bounded, correct fundamental" test
  // suite cannot make. The signal can pass every one of those and still be
  // silent in practice, because all of its energy sits below roughly 150 Hz
  // where ordinary speakers reproduce nothing. Here that shows up as bars only
  // at the far left, and this fails.
  //
  // Averaged over a couple of seconds rather than read once. The exhaust is a
  // pulse train, not a tone, so any single frame of the spectrum is a snapshot
  // of something that moves with every firing event — and at `pullAway`'s full
  // 1800 N·m the engine is also close to losing the fight, so one badly timed
  // read can land on a trough. This was an intermittent failure, not a
  // borderline one: the same reasoning `measureOutput` was written for.
  const heights = await averagedBars(page);

  expect(heights.length).toBeGreaterThan(16);
  const tallest = Math.max(...heights);
  expect(tallest, 'the spectrum should show real output').toBeGreaterThan(1);

  const audible = heights
    .slice(bucketOf(150, heights.length), bucketOf(4_000, heights.length))
    .reduce((max, h) => Math.max(max, h), 0);

  expect(
    audible,
    'most of the exhaust energy must sit above 150 Hz, or nothing will be heard',
  ).toBeGreaterThan(tallest * 0.25);
});

test('the cockpit stage muffles the top end without simply being louder', async ({ page }) => {
  const external = await bootstrap(page);
  await page.getByTestId('start').click();
  await expect(page.getByTestId('run-state')).toHaveText('running', { timeout: 20_000 });

  await page.getByTestId('enable-audio').click();
  await expect(page.getByTestId('spectrum')).toBeVisible({ timeout: 20_000 });

  const raw = page.getByTestId('audio-stage-raw');
  const cockpit = page.getByTestId('audio-stage-cockpit');

  // The cab is the default; raw is one click away.
  await expect(cockpit).toHaveAttribute('aria-pressed', 'true');

  // Let the start transient go before measuring anything. Starting is the
  // loudest thing this engine does — louder than the governed rev limit — so a
  // measurement taken too soon after it is a measurement of the start, and the
  // two stages are measured seconds apart.
  await expect
    .poll(() => rpm(page), { timeout: 20_000, message: 'the engine should settle to idle' })
    .toBeLessThan(700);
  await page.waitForTimeout(2_000);

  // Idle first. Level-matching at one operating point is not level-matching:
  // the compressor in the cab chain works at the loud end and does nothing at
  // the quiet end, so both ends have to be measured or the match is an
  // accident of where it was measured.
  const idleCockpit = await measureOutput(page);
  await raw.click();
  await expect(raw).toHaveAttribute('aria-pressed', 'true');
  await expect(cockpit).toHaveAttribute('aria-pressed', 'false');
  await page.waitForTimeout(500);
  const idleRaw = await measureOutput(page);

  // Then working, where there is enough signal above 2 kHz for the spectrum
  // comparison to be a comparison of content rather than of two noise floors.
  //
  // Deliberately not `pullAway`'s full 1800 N·m. This test sits at its working
  // point for the best part of ten seconds while it measures, and an engine
  // held at maximum load for that long with no gear to drop into eventually
  // loses the fight and stalls. That is a truthful thing for the model to do
  // and a useless thing to try to measure through.
  await page.getByTestId('pedal').fill('90');
  await page.getByTestId('load').fill('300');
  await expect
    .poll(() => rpm(page), { timeout: 30_000, message: 'the engine should pick up speed' })
    .toBeGreaterThan(1_400);
  await page.waitForTimeout(1_500);
  const loadRaw = await measureOutput(page);

  // Guard the comparison below: two silences compare equal, and a stalled
  // engine would otherwise fail as an inscrutable 0 against 0.
  expect(loadRaw.high, 'the raw stage should have content above 2 kHz to compare').toBeGreaterThan(
    0.5,
  );

  await cockpit.click();
  await expect(cockpit).toHaveAttribute('aria-pressed', 'true');
  await page.waitForTimeout(500);
  const loadCockpit = await measureOutput(page);

  // Sitting in a cab takes the sharp edge of blowdown away: glass, insulation
  // and several metres of air are a low pass, and this is that low pass showing
  // up in the output rather than in the source.
  expect(
    loadCockpit.high,
    `the cab should roll the top end off (cockpit ${loadCockpit.high.toFixed(1)} vs raw ${loadRaw.high.toFixed(1)})`,
  ).toBeLessThan(loadRaw.high * 0.75);

  // And it keeps the low end, which is the part of a diesel you feel.
  expect(loadCockpit.low).toBeGreaterThan(loadRaw.low * 0.75);

  // The seat is a low-frequency listening position, and that is a statement
  // about the *ratio* rather than about either band alone — the two stages are
  // level-matched, so "more low" can only mean "more low per unit of high".
  //
  // This is the assertion the per-path level trims exist to satisfy: the body
  // path up, the block down, measured at the output rather than asserted from
  // the spec. Adding them moved this ratio from 3.7 to 4.4 under load while the
  // raw stage stayed at 2.4.
  const cockpitRatio = loadCockpit.low / Math.max(loadCockpit.high, 1e-6);
  const rawRatio = loadRaw.low / Math.max(loadRaw.high, 1e-6);
  expect(
    cockpitRatio,
    `the cab should weight the low end more heavily than raw does ` +
      `(cockpit ${cockpitRatio.toFixed(2)}, raw ${rawRatio.toFixed(2)})`,
  ).toBeGreaterThan(rawRatio * 1.5);

  // Reported rather than only asserted. `CABIN_SPEC.dynamics.makeupGain` and the
  // per-path level trims are calibrated against this spread, and README quotes
  // it; a number that only appears when it fails is a number nobody can tune to.
  console.log(
    `cab spread: idle ${(idleCockpit.levelDb - idleRaw.levelDb).toFixed(1)} dB, ` +
      `load ${(loadCockpit.levelDb - loadRaw.levelDb).toFixed(1)} dB ` +
      `| low idle ${idleCockpit.low.toFixed(1)}/${idleRaw.low.toFixed(1)} ` +
      `load ${loadCockpit.low.toFixed(1)}/${loadRaw.low.toFixed(1)} ` +
      `| high load ${loadCockpit.high.toFixed(1)}/${loadRaw.high.toFixed(1)}`,
  );

  // The switch must not be a volume control in disguise. A post-processing
  // stage that is merely louder wins any comparison for the wrong reason, so
  // the two are level-matched and these are the assertions that hold them
  // there — at both ends of the range.
  expect(
    Math.abs(loadCockpit.levelDb - loadRaw.levelDb),
    `stages should be level-matched under load (cockpit ${loadCockpit.levelDb.toFixed(1)} dBFS, raw ${loadRaw.levelDb.toFixed(1)} dBFS)`,
  ).toBeLessThan(3);
  expect(
    Math.abs(idleCockpit.levelDb - idleRaw.levelDb),
    `stages should be level-matched at idle (cockpit ${idleCockpit.levelDb.toFixed(1)} dBFS, raw ${idleRaw.levelDb.toFixed(1)} dBFS)`,
  ).toBeLessThan(3);

  // Sound keeps flowing across the switch, and the generated impulse response
  // means the cab costs no network request.
  const received = await numeric(page, 'audio-received');
  await raw.click();
  await expect
    .poll(() => numeric(page, 'audio-received'), { timeout: 10_000 })
    .toBeGreaterThan(received);
  expect(external, `unexpected external requests: ${external.join(', ')}`).toEqual([]);
});

test('each radiating path can be heard on its own', async ({ page }) => {
  const external = await bootstrap(page);
  await page.getByTestId('start').click();
  await expect(page.getByTestId('run-state')).toHaveText('running', { timeout: 20_000 });

  await page.getByTestId('enable-audio').click();
  await expect(page.getByTestId('spectrum')).toBeVisible({ timeout: 20_000 });

  // The raw stage, so this measures the three paths rather than the cab filters
  // applied to them.
  await page.getByTestId('audio-stage-raw').click();

  // Deliberately not `pullAway`'s full 1800 N·m, for the reason the cockpit test
  // gives: this sits at its working point for the best part of half a minute
  // while it takes four measurements, and an engine held at maximum load that
  // long with no gear to drop into loses the fight and stalls. Which is truthful
  // of the model and useless to measure through.
  await page.getByTestId('pedal').fill('90');
  await page.getByTestId('load').fill('300');
  await expect
    .poll(() => rpm(page), { timeout: 30_000, message: 'the engine should pick up speed' })
    .toBeGreaterThan(1_400);
  await page.waitForTimeout(1_500);

  const all = await measureOutput(page);
  expect(all.low, 'the engine should be making a sound to begin with').toBeGreaterThan(0);
  expect(all.high, 'and some of it above 2 kHz, to have something to remove').toBeGreaterThan(0.5);

  // Body alone. It is the torque-driven path, so it lives below a few hundred
  // hertz and has essentially nothing up top: silencing the other two should
  // take the high band away and leave the low band standing. If the channels
  // were crossed somewhere between the solver and the graph, this is where it
  // would show — a path would be muted and the wrong content would vanish.
  await page.getByTestId('audio-path-exhaust').click();
  await page.getByTestId('audio-path-block').click();
  await expect(page.getByTestId('audio-path-exhaust')).toHaveAttribute('aria-pressed', 'false');
  await page.waitForTimeout(500);

  const bodyOnly = await measureOutput(page);
  expect(
    bodyOnly.high,
    `the body path should have almost nothing above 2 kHz (was ${all.high.toFixed(1)}, now ${bodyOnly.high.toFixed(1)})`,
  ).toBeLessThan(all.high * 0.5);
  expect(bodyOnly.low, 'and it should still be carrying the low end').toBeGreaterThan(0);

  // Everything muted is silence, which is the check that these are really the
  // whole signal between them and not three views of something else.
  await page.getByTestId('audio-path-body').click();
  await page.waitForTimeout(700);
  const muted = await measureOutput(page);
  expect(muted.levelDb, 'muting every path must leave silence').toBeLessThan(all.levelDb - 20);

  // And it comes back.
  for (const path of ['exhaust', 'block', 'body']) {
    await page.getByTestId(`audio-path-${path}`).click();
    await expect(page.getByTestId(`audio-path-${path}`)).toHaveAttribute('aria-pressed', 'true');
  }
  await page.waitForTimeout(1_000);
  const restored = await measureOutput(page);
  expect(restored.levelDb).toBeGreaterThan(muted.levelDb + 10);

  expect(external, 'no network requests').toEqual([]);
});

test('a gear and a downhill grade drive the engine, and the brake arrests it', async ({ page }) => {
  await bootstrap(page);
  await page.getByTestId('start').click();
  await expect.poll(() => rpm(page), { timeout: 30_000 }).toBeGreaterThan(400);

  // Out of gear the driveline contributes nothing at all, whatever the grade.
  await page.getByTestId('grade').fill('-6');
  expect(await numeric(page, 'torque-driveline')).toBe(0);
  await expect(page.getByTestId('driveline-speed')).toHaveText('—');

  // Get some speed up, then take a gear on the descent and lift off.
  //
  // Fifth, not top. A forty-tonne truck on a six percent grade in a high gear
  // runs away from any engine brake ever built — at 2000 rpm in ninth, gravity
  // is feeding in around 600 kW against the brake's 220 — which is exactly why
  // a driver descends in a low gear. The same road speed at a lower ratio means
  // a faster-turning engine absorbing more, and less gravitational power going
  // in. Choosing a gear where the brake loses would test nothing but arithmetic.
  await pullAway(page);
  await page.getByTestId('load').fill('0');
  await page.getByTestId('gear').fill('5');
  await page.getByTestId('pedal').fill('0');

  await expect
    .poll(() => numeric(page, 'reflected-inertia'), {
      timeout: 20_000,
      message: 'a laden truck in gear must reflect real inertia onto the crank',
    })
    .toBeGreaterThan(1);
  await expect.poll(() => numeric(page, 'vehicle-speed'), { timeout: 20_000 }).toBeGreaterThan(0);
  await expect
    .poll(() => numeric(page, 'torque-driveline'), {
      timeout: 20_000,
      message: 'a descent must drive the crank rather than resist it',
    })
    .toBeLessThan(0);

  // Unbraked, the hill winds the engine up on its own.
  await expect
    .poll(() => rpm(page), {
      timeout: 30_000,
      message: 'a descent in gear with no fuel must accelerate the engine',
    })
    .toBeGreaterThan(1_500);
  const runaway = await rpm(page);

  // Now the brake: all six cylinders plus the wastegate.
  await page.getByTestId('brake-stage-3').click();
  await expect
    .poll(() => numeric(page, 'brake-absorbed'), {
      timeout: 20_000,
      message: 'the engine brake must report the power it is absorbing',
    })
    .toBeGreaterThan(10);
  await expect(page.getByTestId('brake-stage-active')).toHaveText('III');

  await expect
    .poll(() => rpm(page), { timeout: 40_000, message: 'the brake must arrest the descent' })
    .toBeLessThan(runaway);
});

test('the brake reports when it is selected but not permitted to act', async ({ page }) => {
  await bootstrap(page);
  await page.getByTestId('start').click();
  await expect.poll(() => rpm(page), { timeout: 30_000 }).toBeGreaterThan(400);

  // At idle the engine is below the published 1000 rpm floor, so asking for a
  // stage must not silently appear to have worked.
  await page.getByTestId('brake-stage-3').click();
  await expect(page.getByTestId('brake-inhibited')).toBeVisible();
  await expect(page.getByTestId('brake-stage-active')).toHaveText('off');
  expect(await numeric(page, 'brake-absorbed')).toBe(0);

  // Above the floor, with the pedal released, it engages.
  await pullAway(page);
  await page.getByTestId('pedal').fill('0');
  await expect
    .poll(() => page.getByTestId('brake-stage-active').innerText(), { timeout: 20_000 })
    .toBe('III');
  await expect(page.getByTestId('brake-inhibited')).toBeHidden();
});
