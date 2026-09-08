import { expect, test, type Page } from '@playwright/test';
import { chooseOption } from './ui';

declare global {
  interface Window {
    audioTest: { contexts: AudioContext[]; analyser?: AnalyserNode; received: number; failWorklet: boolean };
  }
}

async function setup(page: Page) {
  // Test-owned instrumentation reads the real graph without shipping debug controls.
  await page.addInitScript(() => {
    window.audioTest = { contexts: [], received: 0, failWorklet: false };
    const Original = window.AudioContext;
    window.AudioContext = class extends Original {
      constructor(options?: AudioContextOptions) {
        super(options);
        window.audioTest.contexts.push(this);
        const addModule = this.audioWorklet.addModule.bind(this.audioWorklet);
        this.audioWorklet.addModule = (url, options) => window.audioTest.failWorklet
          ? Promise.reject(new DOMException('Worklet unavailable', 'AbortError'))
          : addModule(url, options);
      }
      override createAnalyser() { const node = super.createAnalyser(); window.audioTest.analyser = node; return node; }
    };
    const OriginalWorklet = window.AudioWorkletNode;
    window.AudioWorkletNode = class extends OriginalWorklet {
      constructor(context: BaseAudioContext, name: string, options?: AudioWorkletNodeOptions) {
        super(context, name, options);
        this.port.addEventListener('message', (event) => {
          if (event.data.type === 'status') window.audioTest.received = event.data.received;
        });
      }
    };
  });
  await page.goto('./');
  await expect(page.getByTestId('status-bar')).toHaveAttribute('data-lifecycle', 'ready');
}

async function level(page: Page) {
  return page.evaluate(() => {
    const analyser = window.audioTest.analyser;
    if (!analyser) return -120;
    const samples = new Float32Array(analyser.fftSize);
    analyser.getFloatTimeDomainData(samples);
    const power = samples.reduce((sum, sample) => sum + sample * sample, 0) / samples.length;
    return 10 * Math.log10(Math.max(power, 1e-12));
  });
}

test('engine start automatically plays sound and retains position and volume across views', async ({ page }) => {
  await setup(page);
  expect(await page.evaluate(() => window.audioTest.contexts.length)).toBe(0);
  await page.getByTestId('start').click();
  await expect.poll(async () => Number(await page.getByTestId('rpm').textContent())).toBeGreaterThan(500);
  await expect(page.getByTestId('enable-audio')).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.audioTest.received)).toBeGreaterThan(10_000);
  await expect.poll(() => level(page)).toBeGreaterThan(-70);
  await page.getByTestId('nav-sound').click();
  await expect(page.getByTestId('audio-stage-cockpit')).toHaveAttribute('aria-pressed', 'true');
  await page.getByTestId('audio-stage-raw').click();
  await page.getByTestId('audio-volume').fill('0');
  await expect.poll(() => level(page)).toBeLessThan(-90);
  await page.getByTestId('audio-volume').fill('65');
  await expect.poll(() => level(page)).toBeGreaterThan(-70);
  await page.getByTestId('nav-drive').click();
  await page.getByTestId('nav-sound').click();
  await expect(page.getByTestId('audio-stage-raw')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByTestId('audio-volume')).toHaveValue('65');
  expect(await page.evaluate(() => window.audioTest.contexts.length)).toBe(1);
  await page.getByTestId('stop').click();
  await expect.poll(async () => Number(await page.getByTestId('rpm').textContent())).toBe(0);
  await expect.poll(() => level(page)).toBeLessThan(-80);
});

test('failed sound startup closes its context and can be retried', async ({ page }) => {
  await setup(page);
  await chooseOption(page, 'language-select', 'zh-CN');
  await page.evaluate(() => { window.audioTest.failWorklet = true; });
  await page.getByTestId('start').click();
  await expect(page.getByTestId('audio-error')).toContainText('无法播放声音');
  await expect.poll(() => page.evaluate(() => window.audioTest.contexts[0]!.state)).toBe('closed');
  await page.evaluate(() => { window.audioTest.failWorklet = false; });
  await page.getByTestId('audio-error').getByRole('button').click();
  await expect.poll(() => page.evaluate(() => window.audioTest.contexts.at(-1)!.state)).toBe('running');
  await expect(page.getByTestId('audio-error')).toHaveCount(0);
});

test('device suspension is visible and a sound gesture restores playback', async ({ page }) => {
  await setup(page);
  await page.getByTestId('start').click();
  await expect.poll(async () => Number(await page.getByTestId('rpm').textContent())).toBeGreaterThan(500);
  await expect.poll(() => page.evaluate(() => window.audioTest.contexts.at(-1)!.state)).toBe('running');
  await page.evaluate(() => window.audioTest.contexts[0]!.suspend());
  await expect(page.getByTestId('resume-sound')).toBeVisible();
  await page.getByTestId('resume-sound').click();
  await expect.poll(() => page.evaluate(() => window.audioTest.contexts.at(-1)!.state)).toBe('running');
  await expect.poll(() => page.evaluate(() => window.audioTest.contexts.filter(context => context.state === 'running').length)).toBe(1);
});

async function measureOutput(page: Page) {
  const samples: { low: number; high: number; power: number }[] = [];
  for (let i = 0; i < 10; i++) {
    await page.waitForTimeout(200);
    samples.push(await page.evaluate(() => {
      const analyser = window.audioTest.analyser!;
      const bins = new Uint8Array(analyser.frequencyBinCount);
      analyser.getByteFrequencyData(bins);
      const buckets = Array.from({ length: 64 }, () => ({ sum: 0, count: 0 }));
      for (let bin = 1; bin < bins.length; bin++) {
        const hz = bin * analyser.context.sampleRate / analyser.fftSize;
        if (hz < 20 || hz > 20000) continue;
        const bucket = buckets[Math.min(63, Math.floor(Math.log10(hz / 20) / 3 * 64))]!;
        bucket.sum += bins[bin]!;
        bucket.count++;
      }
      const band = (low: number, high: number) => {
        const start = Math.floor(Math.log10(low / 20) / 3 * 64);
        const end = Math.floor(Math.log10(high / 20) / 3 * 64);
        const values = buckets.slice(start, end).map(({ sum, count }) => count ? sum / count : 0);
        return values.reduce((a, b) => a + b, 0) / values.length;
      };
      const wave = new Float32Array(analyser.fftSize);
      analyser.getFloatTimeDomainData(wave);
      return { low: band(60, 400), high: band(2000, 8000), power: wave.reduce((sum, v) => sum + v * v, 0) / wave.length };
    }));
  }
  const mean = (key: 'low' | 'high' | 'power') => samples.reduce((sum, s) => sum + s[key], 0) / samples.length;
  return { low: mean('low'), high: mean('high'), db: 10 * Math.log10(mean('power')) };
}

test('cockpit and raw remain level-matched while the cab softens high frequencies', async ({ page }) => {
  await setup(page);
  await page.getByTestId('start').click();
  await expect.poll(async () => Number(await page.getByTestId('rpm').textContent())).toBeGreaterThan(500);
  await page.getByTestId('nav-sound').click();
  await page.waitForTimeout(2000);
  const idleCab = await measureOutput(page);
  await page.getByTestId('audio-stage-raw').click();
  await page.waitForTimeout(500);
  const idleRaw = await measureOutput(page);
  await page.getByTestId('pedal').fill('90');
  await page.getByTestId('load').fill('300');
  await expect.poll(async () => Number(await page.getByTestId('rpm').textContent())).toBeGreaterThan(1400);
  await page.waitForTimeout(1500);
  const loadedRaw = await measureOutput(page);
  await page.getByTestId('audio-stage-cockpit').click();
  await page.waitForTimeout(500);
  const loadedCab = await measureOutput(page);
  expect(loadedRaw.high).toBeGreaterThan(0.5);
  expect(loadedCab.high).toBeLessThan(loadedRaw.high * .75);
  expect(loadedCab.low).toBeGreaterThan(loadedRaw.low * .75);
  expect(loadedCab.low / loadedCab.high).toBeGreaterThan(loadedRaw.low / loadedRaw.high * 1.5);
  expect(Math.abs(idleCab.db - idleRaw.db)).toBeLessThan(3);
  expect(Math.abs(loadedCab.db - loadedRaw.db)).toBeLessThan(3);
});
