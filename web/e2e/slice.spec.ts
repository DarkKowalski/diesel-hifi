import { expect, test, type Page } from '@playwright/test';
import { chooseOption } from './ui';

const ENGINE_ID = 'mercedes-benz-om471-9-m3d-375kw';

async function bootstrap(page: Page) {
  await page.goto('./');
  await expect(page.getByTestId('status-bar')).toHaveAttribute('data-lifecycle', 'ready');
}
async function numeric(page: Page, id: string) {
  return Number.parseFloat(await page.getByTestId(id).textContent() ?? 'NaN');
}
async function openControls(page: Page) {
  const toggle = page.getByTestId('controls-toggle');
  if (await toggle.isVisible() && await toggle.getAttribute('aria-expanded') === 'false') await toggle.click();
}
async function start(page: Page) {
  await openControls(page);
  await page.getByTestId('start').click();
  await expect(page.getByTestId('run-state')).toHaveAttribute('data-state', 'running');
  await expect.poll(() => numeric(page, 'rpm'), { timeout: 30_000 }).toBeGreaterThan(500);
}

test('selection menus support keyboard, outside dismissal and localized mobile options', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await bootstrap(page);
  const language = page.getByTestId('language-select');
  await language.focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('option', { name: 'English' })).toHaveAttribute('aria-selected', 'true');
  await page.screenshot({ path: testInfo.outputPath('language-menu.png') });
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  await expect(page.locator('html')).toHaveAttribute('lang', 'zh-CN');
  await expect(language).toHaveText('简体中文');
  await language.click();
  await page.keyboard.press('Home');
  await page.keyboard.press('Escape');
  await expect(language).toHaveAttribute('data-value', 'zh-CN');
  await expect(page.getByRole('listbox')).toHaveCount(0);
  await language.click();
  await page.getByTestId('engine-select').click();
  await expect(page.getByRole('listbox')).toHaveCount(1);
  await expect(page.getByRole('option')).toContainText('参考模型');
  const menu = (await page.getByRole('listbox').boundingBox())!;
  expect(menu.x).toBeGreaterThanOrEqual(0);
  expect(menu.x + menu.width).toBeLessThanOrEqual(390);
  await page.screenshot({ path: testInfo.outputPath('engine-menu.png') });
  await page.getByTestId('speed-panel').click({ position: { x: 5, y: 5 } });
  await expect(page.getByRole('listbox')).toHaveCount(0);
  await expect(page.locator('select')).toHaveCount(0);
  for (const locale of ['en', 'zh-CN']) {
    await chooseOption(page, 'language-select', locale);
    const selector = (await page.getByTestId('engine-select').boundingBox())!;
    const specs = (await page.getByTestId('specs-toggle').boundingBox())!;
    expect(Math.abs(selector.y - specs.y)).toBeLessThan(1);
    expect(Math.abs(selector.height - specs.height)).toBeLessThan(1);
    await page.getByTestId('specs-toggle').click();
    await expect(page.getByTestId('engine-spec')).toBeVisible();
    await page.getByTestId('specs-toggle').click();
    await expect(page.getByTestId('engine-spec')).toBeHidden();
  }
});

test('Diesel HiFi loads its catalog without production debugging tools or external requests', async ({ page }) => {
  const external: string[] = [];
  page.on('request', (request) => {
    const url = new URL(request.url());
    if (!['localhost', '127.0.0.1'].includes(url.hostname) && !['blob:', 'data:'].includes(url.protocol)) external.push(url.href);
  });
  await bootstrap(page);
  await expect(page).toHaveTitle('Diesel HiFi');
  await expect(page.getByTestId('engine-select')).toHaveAttribute('data-value', ENGINE_ID);
  await expect(page.getByTestId('engine-select')).toHaveAttribute('data-source', 'catalog-api');
  await page.getByTestId('engine-select').click();
  await expect(page.getByRole('option')).toHaveCount(1);
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('engine-spec')).toContainText('Published');
  for (const view of ['drive', 'sound', 'details']) {
    await page.getByTestId(`nav-${view}`).click();
    for (const id of ['run-sweep', 'provenance-entries', 'spectrum', 'audio-paths', 'audio-compare', 'egr-toggle', 'enable-audio', 'disable-audio']) {
      await expect(page.getByTestId(id)).toHaveCount(0);
    }
  }
  await start(page);
  expect(external).toEqual([]);
});

test('engine start, accelerator, release, stop and restart reach the real simulation', async ({ page }) => {
  await bootstrap(page);
  await expect(page.getByTestId('rpm')).toHaveText('0');
  await expect(page.getByTestId('run-state')).toHaveAttribute('data-state', 'stopped');
  await start(page);
  await page.getByTestId('pedal').fill('65');
  await expect.poll(() => numeric(page, 'rpm')).toBeGreaterThan(1500);
  await page.getByTestId('release-pedal').click();
  await expect(page.getByTestId('pedal')).toHaveValue('0');
  await expect.poll(() => numeric(page, 'rpm'), { timeout: 30_000 }).toBeLessThan(650);
  await page.getByTestId('stop').click();
  await expect.poll(() => numeric(page, 'rpm'), { timeout: 30_000 }).toBe(0);
  await start(page);
  await page.getByTestId('reset').click();
  await expect(page.getByTestId('rpm')).toHaveText('0');
  await expect(page.getByTestId('pedal')).toHaveValue('0');
  await expect(page.getByTestId('run-state')).toHaveAttribute('data-state', 'stopped');
});

test('Drive cutaway animates with the engine and respects reset, navigation and reduced motion', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 1200 });
  await bootstrap(page);
  const engine = page.getByTestId('engine-animation');
  const piston = page.getByTestId('engine-piston').first();
  await expect(engine).toBeVisible();
  await expect(page.getByTestId('engine-piston')).toHaveCount(6);
  await expect(engine).toHaveAttribute('data-moving', 'false');
  const initial = await piston.getAttribute('transform');
  await expect(page.getByTestId('audio-volume')).toHaveCount(0);
  await start(page);
  await expect(engine).toHaveAttribute('data-moving', 'true');
  await expect.poll(() => piston.getAttribute('transform')).not.toBe(initial);
  await page.getByTestId('reset').click();
  await expect(engine).toHaveAttribute('data-moving', 'false');
  await expect(piston).toHaveAttribute('transform', initial!);
  await start(page);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await expect(engine).toHaveAttribute('data-moving', 'false');
  await expect(page.getByRole('switch', { name: 'Slow motion' })).toBeDisabled();
  const frozen = await piston.getAttribute('transform');
  await page.waitForTimeout(200);
  await expect(piston).toHaveAttribute('transform', frozen!);
  await page.getByTestId('nav-sound').click();
  await expect(engine).toHaveCount(0);
  await expect(page.getByTestId('audio-volume')).toBeVisible();
});

test('speed pointer stays on the rail and tracks live RPM during acceleration and reset', async ({ page }) => {
  // Observe real worker snapshots from the test, without a production debug API.
  await page.addInitScript(() => {
    const state = window as typeof window & { pointerRpm: number };
    state.pointerRpm = 0;
    const NativeWorker = window.Worker;
    window.Worker = class extends NativeWorker {
      constructor(url: string | URL, options?: WorkerOptions) {
        super(url, options);
        this.addEventListener('message', ({ data }) => {
          if (data.t === 'snapshot') state.pointerRpm = data.snapshot.rpm;
        });
      }
    };
  });
  await bootstrap(page);
  await start(page);
  const samplePointer = (duration: number) => page.evaluate(async (durationMs) => {
    const marker = document.querySelector<SVGCircleElement>('[data-testid="rpm-pointer"]')!;
    const rail = document.querySelector<SVGPathElement>('[data-testid="rpm-rail"]')!;
    const svg = marker.ownerSVGElement!;
    const length = rail.getTotalLength();
    const until = performance.now() + durationMs;
    let maxError = 0;
    let frames = 0;
    let maxRpm = 0;
    do {
      await new Promise(requestAnimationFrame);
      const rpm = (window as typeof window & { pointerRpm: number }).pointerRpm;
      const style = getComputedStyle(marker);
      const matrix = svg.getScreenCTM()!.inverse().multiply(marker.getScreenCTM()!);
      const actual = new DOMPoint(parseFloat(style.cx), parseFloat(style.cy)).matrixTransform(matrix);
      const expected = rail.getPointAtLength(Math.max(0, Math.min(1, rpm / 2500)) * length);
      maxError = Math.max(maxError, Math.hypot(actual.x - expected.x, actual.y - expected.y));
      maxRpm = Math.max(maxRpm, rpm);
      frames++;
    } while (performance.now() < until);
    return { maxError, frames, maxRpm };
  }, duration);
  await page.getByTestId('pedal').fill('90');
  const acceleration = await samplePointer(900);
  expect(acceleration.frames).toBeGreaterThan(10);
  expect(acceleration.maxRpm).toBeGreaterThan(1000);
  expect(acceleration.maxError).toBeLessThan(2);
  const resetting = samplePointer(300);
  await page.getByTestId('reset').click();
  expect((await resetting).maxError).toBeLessThan(2);
  await expect(page.getByTestId('rpm')).toHaveText('0');
});

test('slow motion switch changes drawing speed and persists across views and languages', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 1200 });
  await bootstrap(page);
  const toggle = page.getByTestId('engine-slow-motion');
  await expect(toggle).toHaveAttribute('aria-checked', 'true');
  await start(page);
  const angularSpeed = () => page.evaluate(async () => {
    const pin = document.querySelector<SVGCircleElement>('[data-testid="engine-crank-pin"]')!;
    const center = pin.nextElementSibling as SVGCircleElement;
    const angle = () => Math.atan2(pin.cx.baseVal.value - center.cx.baseVal.value,
      center.cy.baseVal.value - pin.cy.baseVal.value);
    let previous = angle();
    let traveled = 0;
    const started = performance.now();
    do {
      await new Promise(requestAnimationFrame);
      const current = angle();
      traveled += Math.atan2(Math.sin(current - previous), Math.cos(current - previous));
      previous = current;
    } while (performance.now() - started < 600);
    return traveled / (performance.now() - started) * 1000;
  });
  const slow = await angularSpeed();
  expect(slow).toBeGreaterThan(1);
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-checked', 'false');
  const fullSpeed = await angularSpeed();
  expect(fullSpeed / slow).toBeGreaterThan(7);
  expect(fullSpeed / slow).toBeLessThan(13);
  expect(await numeric(page, 'rpm')).toBeLessThan(650);
  await page.getByTestId('nav-sound').click();
  await page.getByTestId('nav-drive').click();
  await expect(toggle).toHaveAttribute('aria-checked', 'false');
  await chooseOption(page, 'language-select', 'zh-CN');
  await expect(page.getByRole('switch', { name: '慢动作' })).toHaveAttribute('aria-checked', 'false');
  await page.getByTestId('reset').click();
  await expect(toggle).toHaveAttribute('aria-checked', 'false');
  await toggle.focus();
  await page.keyboard.press('Space');
  await expect(toggle).toHaveAttribute('aria-checked', 'true');
});

test('telemetry is separate from driving and keeps pressure, torque, starter and air-path readings', async ({ page }) => {
  await bootstrap(page);
  await expect(page.getByTestId('cycle-averages')).toBeHidden();
  await start(page);
  await page.getByTestId('nav-details').click();
  await expect(page.getByTestId('cycle-averages')).toBeVisible();
  await expect.poll(() => numeric(page, 'brake-power')).not.toBeNaN();
  await page.getByTestId('telemetry-starter-data').locator('summary').click();
  await expect(page.getByTestId('starter-current')).toBeVisible();
  await expect.poll(() => numeric(page, 'starter-current')).toBe(0);
  await expect.poll(() => numeric(page, 'starter-volts')).toBeGreaterThan(20);
  await page.getByTestId('telemetry-cylinders').locator('summary').click();
  await expect(page.getByTestId('cylinders').locator('li')).toHaveCount(6);
  await expect.poll(() => numeric(page, 'peak-session')).toBeGreaterThan(1);
  await page.getByTestId('telemetry-air-path').locator('summary').click();
  await page.getByTestId('pedal').fill('80');
  await expect.poll(() => numeric(page, 'turbo-shaft')).toBeGreaterThan(5);
});

test('catalog selection resets the session and controls', async ({ page }) => {
  await bootstrap(page);
  await start(page);
  await page.getByTestId('pedal').fill('40');
  await chooseOption(page, 'engine-select', ENGINE_ID);
  await expect(page.getByTestId('rpm')).toHaveText('0');
  await expect(page.getByTestId('pedal')).toHaveValue('0');
  await expect(page.getByTestId('run-state')).toHaveAttribute('data-state', 'stopped');
  await page.getByTestId('nav-details').click();
  await expect(page.getByTestId('sim-time')).toHaveText('0.00 s');
});

test('a downhill gear drives the crank and engine braking slows it', async ({ page }) => {
  await bootstrap(page);
  await start(page);
  await page.getByTestId('brake-stage-3').click();
  await expect(page.getByTestId('brake-inhibited')).toBeVisible();
  await page.getByTestId('brake-stage-0').click();
  await page.getByTestId('pedal').fill('90');
  await expect.poll(() => numeric(page, 'rpm')).toBeGreaterThan(1400);
  await page.getByTestId('grade').fill('-6');
  await page.getByTestId('gear').fill('5');
  await page.getByTestId('release-pedal').click();
  await expect.poll(() => numeric(page, 'rpm')).toBeGreaterThan(1500);
  const unbraked = await numeric(page, 'rpm');
  await page.getByTestId('brake-stage-3').click();
  await expect(page.getByTestId('brake-engaged')).toBeVisible();
  await expect.poll(() => numeric(page, 'rpm'), { timeout: 40_000 }).toBeLessThan(unbraked);
  await page.getByTestId('nav-details').click();
  await page.getByTestId('telemetry-brake-driveline').locator('summary').click();
  await expect.poll(() => numeric(page, 'brake-absorbed')).toBeGreaterThan(10);
  await expect.poll(() => numeric(page, 'vehicle-speed')).toBeGreaterThan(0);
});

for (const mobile of [false, true]) {
test(`overspeed offers one-click grade recovery with preserved time (${mobile ? 'phone' : 'desktop'})`, async ({ page }) => {
  if (mobile) await page.setViewportSize({ width: 390, height: 844 });
  await bootstrap(page);
  await chooseOption(page, 'language-select', 'zh-CN');
  await start(page);
  await page.getByTestId('pedal').fill('100');
  await expect.poll(() => numeric(page, 'rpm')).toBeGreaterThan(1950);
  await page.getByTestId('grade').fill('-15');
  await page.getByTestId('gear').fill('12');
  await page.getByTestId('release-pedal').click();
  await expect(page.getByTestId('speed-limit-banner')).toBeVisible({ timeout: 30_000 });
  await expect(page.getByTestId('speed-limit-banner')).toContainText('发动机转速超限');
  await expect(page.getByTestId('speed-limit-banner')).toContainText('将道路坡度重置为 0%，然后继续模拟？');
  await expect(page.getByTestId('start')).toBeDisabled();
  const pausedTime = await numeric(page, 'sim-time');
  await expect(page.getByTestId('grade')).toHaveValue('-15');
  await expect(page.getByTestId('resume')).toHaveText('重置坡度并继续');
  await page.getByTestId('resume').click();
  await expect(page.getByTestId('speed-limit-banner')).toHaveCount(0);
  await expect(page.getByTestId('grade')).toHaveValue('0');
  await expect(page.getByTestId('gear')).toHaveValue('12');
  await expect.poll(() => numeric(page, 'rpm')).toBeLessThan(2380);
  await expect.poll(() => numeric(page, 'sim-time')).toBeGreaterThan(pausedTime);
});
}

test('language switching persists and does not restart the engine or sound preferences', async ({ page }) => {
  await bootstrap(page);
  await start(page);
  await page.getByTestId('pedal').fill('35');
  await page.getByTestId('nav-sound').click();
  await page.getByTestId('audio-stage-raw').click();
  await page.getByTestId('audio-volume').fill('42');
  await chooseOption(page, 'language-select', 'zh-CN');
  await expect(page.locator('html')).toHaveAttribute('lang', 'zh-CN');
  await expect(page).toHaveTitle('Diesel HiFi');
  await expect(page.getByTestId('start')).toHaveText('启动发动机');
  await expect(page.getByTestId('audio-stage-raw')).toContainText('原始发动机混音');
  await expect(page.getByTestId('pedal')).toHaveValue('35');
  await expect(page.getByTestId('audio-volume')).toHaveValue('42');
  await expect(page.getByTestId('run-state')).toHaveAttribute('data-state', 'running');
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('lang', 'zh-CN');
  await expect(page.getByTestId('language-select')).toHaveAttribute('data-value', 'zh-CN');
  await expect(page.getByTestId('rpm')).toHaveText('0');
});

test('Chinese browser detection works when local storage is unavailable', async ({ browser, baseURL }) => {
  const context = await browser.newContext({ baseURL, locale: 'zh-CN' });
  await context.addInitScript(() => {
    Storage.prototype.getItem = () => { throw new DOMException('Blocked', 'SecurityError'); };
    Storage.prototype.setItem = () => { throw new DOMException('Blocked', 'SecurityError'); };
  });
  const page = await context.newPage();
  await bootstrap(page);
  await expect(page.locator('html')).toHaveAttribute('lang', 'zh-CN');
  await chooseOption(page, 'language-select', 'en');
  await expect(page.getByTestId('start')).toHaveText('Start engine');
  await start(page);
  await context.close();
});

test('loading failure gives a localized recovery action', async ({ browser, baseURL }) => {
  const context = await browser.newContext({ baseURL, locale: 'zh-CN' });
  const page = await context.newPage();
  await page.route('**/*.wasm', (route) => route.abort());
  await page.goto('./');
  await expect(page.getByTestId('error-banner')).toContainText('无法加载发动机');
  await expect(page.getByRole('button', { name: '刷新页面' })).toBeVisible();
  await expect(page.getByTestId('start')).toBeDisabled();
  await context.close();
});

for (const locale of ['en', 'zh-CN'] as const) {
  test(`responsive layout, touch targets and keyboard controls (${locale})`, async ({ page }, testInfo) => {
    await bootstrap(page);
    await chooseOption(page, 'language-select', locale);
    await page.emulateMedia({ reducedMotion: 'reduce' });
    for (const width of [360, 390, 768, 1440]) {
      await page.setViewportSize({ width, height: width > 760 ? 1050 : 844 });
      for (const view of ['drive', 'sound', 'details']) {
        await page.getByTestId(`nav-${view}`).click();
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), `${view} at ${width}`).toBe(true);
        await expect(page.getByTestId(`nav-${view}`)).toHaveAttribute('aria-current', 'page');
        const box = await page.getByTestId(`nav-${view}`).boundingBox();
        expect(box!.height).toBeGreaterThanOrEqual(44);
      }
      await page.getByTestId('nav-drive').click();
      await openControls(page);
      await page.getByTestId('pedal').focus();
      await page.keyboard.press('ArrowRight');
      await expect(page.getByTestId('pedal')).not.toHaveValue('0');
      await page.getByTestId('release-pedal').click();
      if (width < 768) await page.getByTestId('controls-toggle').click();
      await page.evaluate(() => scrollTo(0, 0));
      if (width === 390 || width === 1440) await page.screenshot({ path: testInfo.outputPath(`drive-${locale}-${width}.png`), fullPage: true });
    }
    await page.setViewportSize({ width: 390, height: 844 });
    await page.getByTestId('nav-sound').click();
    await page.getByTestId('audio-stage-raw').click();
    await expect(page.getByTestId('audio-stage-raw')).toHaveAttribute('aria-pressed', 'true');
    await page.screenshot({ path: testInfo.outputPath(`sound-${locale}-390.png`), fullPage: true });
  });

  test(`mobile speed stays visible while scrolling without hiding controls (${locale})`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width: 390, height: 640 });
    await bootstrap(page);
    await chooseOption(page, 'language-select', locale);
    const bar = page.getByTestId('mobile-speed-bar');
    await expect(bar).toBeHidden();
    const pageHeight = await page.evaluate(() => document.documentElement.scrollHeight);
    await page.getByTestId('rpm').evaluate((readout) => scrollBy(0, readout.getBoundingClientRect().top - 48));
    await expect(bar).toBeVisible();
    await expect.poll(async () => Math.round((await bar.boundingBox())?.y ?? -1)).toBe(0);
    expect(await page.evaluate(() => document.documentElement.scrollHeight)).toBe(pageHeight);
    const strip = await bar.boundingBox();
    expect(strip!.y).toBe(0);
    expect(strip!.height).toBeLessThanOrEqual(80);
    await start(page);
    const pedal = page.getByTestId('pedal');
    await pedal.evaluate((input) => input.scrollIntoView({ block: 'center' }));
    await pedal.fill('85');
    await expect.poll(() => numeric(page, 'mobile-rpm')).toBeGreaterThan(1400);
    await expect(page.getByTestId('mobile-run-state')).toHaveAttribute('data-state', 'running');
    await page.screenshot({ path: testInfo.outputPath(`mobile-speed-${locale}.png`) });
    const pedalHit = await pedal.evaluate((input) => {
      const rect = input.getBoundingClientRect();
      const hit = document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2);
      return { unobstructed: hit === input, coveringElement: hit?.outerHTML.slice(0, 200), top: rect.top, bottom: rect.bottom };
    });
    expect(pedalHit.unobstructed, JSON.stringify(pedalHit)).toBe(true);
    expect(await page.evaluate(() => document.querySelector('[data-testid="mobile-rpm"]')!.textContent
      === document.querySelector('[data-testid="rpm"]')!.textContent)).toBe(true);
    for (const view of ['sound', 'details']) {
      await page.getByTestId(`nav-${view}`).click();
      await expect(bar).toBeHidden();
      await openControls(page);
      await expect(bar).toBeHidden();
      const panel = (await page.getByTestId('speed-panel').boundingBox())!;
      await expect.poll(async () => (await page.getByTestId('controls-drawer').boundingBox())!.y).toBeGreaterThan(panel.y + panel.height);
      await page.getByTestId('rpm').evaluate((readout) => scrollBy(0, readout.getBoundingClientRect().top - 48));
      await expect(bar).toBeVisible();
    }
    await page.getByTestId('reset').click();
    await expect(page.getByTestId('mobile-rpm')).toHaveText('0');
    await expect(page.getByTestId('mobile-run-state')).toHaveAttribute('data-state', 'stopped');
    await page.getByTestId('controls-toggle').click();
    await page.evaluate(() => scrollTo(0, 0));
    await expect(bar).toBeHidden();
    await page.setViewportSize({ width: 740, height: 390 });
    await openControls(page);
    await page.getByTestId('grade').scrollIntoViewIfNeeded();
    await expect(bar).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.setViewportSize({ width: 768, height: 1050 });
    await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight));
    await expect(bar).toBeHidden();
  });

  test(`mobile drawer folds, animates and keeps controls reachable (${locale})`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await bootstrap(page);
    await chooseOption(page, 'language-select', locale);
    const drawer = page.getByTestId('controls-drawer');
    const toggle = page.getByTestId('controls-toggle');
    const body = page.locator('#controls-body');
    const bar = page.getByTestId('mobile-speed-bar');
    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await expect(body).toHaveAttribute('inert', '');
    const collapsed = (await drawer.boundingBox())!;
    const expandedGaugeHeight = (await page.getByTestId('speed-panel').boundingBox())!.height;
    const nav = (await page.getByRole('navigation').boundingBox())!;
    expect(Math.abs(collapsed.y + collapsed.height - nav.y)).toBeLessThan(2);
    expect(collapsed.height).toBeLessThan(80);
    const motion = page.evaluate(async () => {
      const drawer = document.querySelector('[data-testid="controls-drawer"]')!;
      const gauge = document.querySelector('[data-testid="speed-panel"]')!;
      const frames: { height: number; gaugeHeight: number }[] = [];
      const until = performance.now() + 600;
      do {
        await new Promise(requestAnimationFrame);
        frames.push({ height: drawer.getBoundingClientRect().height, gaugeHeight: gauge.getBoundingClientRect().height });
      } while (performance.now() < until);
      return frames;
    });
    await toggle.click();
    const frames = await motion;
    const expanded = (await drawer.boundingBox())!;
    expect(expanded.height).toBeGreaterThan(collapsed.height + 200);
    expect(frames.some((frame) => frame.height > collapsed.height + 10 && frame.height < expanded.height - 10)).toBe(true);
    const compactGaugeHeight = (await page.getByTestId('speed-panel').boundingBox())!.height;
    expect(compactGaugeHeight).toBeLessThan(120);
    expect(frames.some((frame) => frame.gaugeHeight > compactGaugeHeight + 10 && frame.gaugeHeight < expandedGaugeHeight - 10)).toBe(true);
    await expect(bar).toBeHidden();
    const compactGauge = (await page.getByTestId('speed-panel').boundingBox())!;
    expect(expanded.y).toBeGreaterThan(compactGauge.y + compactGauge.height);
    await page.getByTestId('rpm').click({ trial: true });
    await start(page);
    await page.getByTestId('pedal').fill('35');
    await page.screenshot({ path: testInfo.outputPath(`drawer-controls-${locale}.png`) });
    const pageScroll = await page.evaluate(() => scrollY);
    await page.getByTestId('controls-scroll').evaluate((scroller) => { scroller.scrollTop = scroller.scrollHeight; });
    expect(await page.evaluate(() => scrollY)).toBe(pageScroll);
    await page.getByTestId('grade').fill('-2');
    await page.getByTestId('load').fill('150');
    await page.getByTestId('grade').click({ trial: true });
    await page.screenshot({ path: testInfo.outputPath(`drawer-open-${locale}.png`) });
    await page.keyboard.press('Escape');
    await expect(toggle).toBeFocused();
    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await expect(body).toBeHidden();
    await expect(bar).toBeHidden();
    await page.screenshot({ path: testInfo.outputPath(`drawer-folded-${locale}.png`) });
    await toggle.click();
    await expect(page.getByTestId('pedal')).toHaveValue('35');
    await expect(page.getByTestId('grade')).toHaveValue('-2');
    await expect(page.getByTestId('load')).toHaveValue('150');
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await toggle.click();
    expect(await body.evaluate((element) => element.getAnimations().length)).toBe(0);
    await expect(body).toBeHidden();
    await page.setViewportSize({ width: 1440, height: 1050 });
    await expect(toggle).toBeHidden();
    await expect(body).not.toHaveAttribute('inert', '');
    await expect(page.getByTestId('pedal')).toBeVisible();
    await expect(bar).toBeHidden();
  });
}

test('all driving inputs share one panel and rapid accelerator/load updates are preserved', async ({ page }) => {
  await bootstrap(page);
  const controls = page.locator('#controls');
  for (const id of ['pedal', 'gear', 'brake-stage-3', 'load', 'grade']) {
    await expect(controls.getByTestId(id)).toBeVisible();
  }
  await start(page);
  await page.getByTestId('pedal').fill('90');
  await page.getByTestId('load').fill('500');
  await expect(page.getByTestId('pedal')).toHaveValue('90');
  await expect(page.getByTestId('load')).toHaveValue('500');
  await expect.poll(() => numeric(page, 'rpm')).toBeGreaterThan(1100);
  await page.getByTestId('nav-details').click();
  await expect(controls.getByTestId('load')).toBeVisible();
  await expect(page.getByTestId('load')).toHaveValue('500');
  await page.getByTestId('reset').click();
  await expect(page.getByTestId('load')).toHaveValue('0');
  await expect(page.getByTestId('gear')).toHaveValue('0');
});

test('a touch phone can start, accelerate, shift and select the engine brake', async ({ browser, baseURL }) => {
  const context = await browser.newContext({ baseURL, viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, deviceScaleFactor: 2, locale: 'zh-CN' });
  const page = await context.newPage();
  await bootstrap(page);
  await page.getByTestId('controls-toggle').tap();
  await page.getByTestId('start').tap();
  await expect.poll(() => numeric(page, 'rpm')).toBeGreaterThan(500);
  const pedal = page.getByTestId('pedal');
  await pedal.scrollIntoViewIfNeeded();
  const box = await pedal.boundingBox();
  await pedal.tap({ position: { x: box!.width * .6, y: box!.height / 2 } });
  await expect.poll(async () => Number(await pedal.inputValue())).toBeGreaterThan(40);
  await page.getByRole('button', { name: '升挡' }).tap();
  await expect(page.getByTestId('gear')).toHaveValue('1');
  await page.getByTestId('brake-stage-2').tap();
  await expect(page.getByTestId('brake-stage-2')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByTestId('brake-inhibited')).toBeVisible();
  await page.getByTestId('nav-sound').tap();
  await expect(page.getByTestId('audio-volume')).toBeVisible();
  await expect(page.getByTestId('enable-audio')).toHaveCount(0);
  await context.close();
});
