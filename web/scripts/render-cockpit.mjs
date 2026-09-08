// Development-only export through the shipped Web Audio graph. No runtime server.
import { readFile, writeFile, stat } from 'node:fs/promises';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { chromium } from '@playwright/test';
import { createServer } from 'vite';

const [input, rateArg = '48000'] = process.argv.slice(2);
const rate = Number(rateArg);
if (!input || ![44100, 48000].includes(rate)) {
  throw new Error('Usage: pnpm audio:render <capture directory or paths.wav> [44100|48000]');
}
const inputPath = resolve(input);
let files = [inputPath];
if ((await stat(inputPath)).isDirectory()) {
  try {
    const manifest = JSON.parse(await readFile(join(inputPath, 'manifest.json'), 'utf8'));
    files = manifest.scenarios.map((scenario) => join(inputPath, scenario.id, 'paths.wav'));
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
    files = [join(inputPath, 'paths.wav')];
  }
}

const server = await createServer({
  root: fileURLToPath(new URL('../', import.meta.url)),
  server: { host: '127.0.0.1', port: 0, open: false, hmr: false },
  logLevel: 'error',
});
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage();
  // Load only the graph modules; do not start the interactive app or its worker.
  await page.route(server.resolvedUrls.local[0], (route) => route.fulfill({
    contentType: 'text/html', body: '<!doctype html><title>Audio export</title>',
  }));
  await page.goto(server.resolvedUrls.local[0]);
  for (const file of files) {
    const bytes = await readFile(file);
    // audio_capture writes this canonical IEEE-float header and channel order.
    if (bytes.length < 44 || bytes.toString('ascii', 0, 4) !== 'RIFF'
        || bytes.toString('ascii', 8, 12) !== 'WAVE'
        || bytes.toString('ascii', 12, 16) !== 'fmt '
        || bytes.readUInt32LE(16) !== 16 || bytes.readUInt16LE(20) !== 3
        || bytes.readUInt16LE(22) !== 4 || bytes.readUInt16LE(34) !== 32
        || bytes.toString('ascii', 36, 40) !== 'data'
        || bytes.readUInt32LE(40) !== bytes.length - 44 || (bytes.length - 44) % 16) {
      throw new Error(`${file}: expected the four-channel float paths.wav from audio_capture`);
    }
    const sourceRate = bytes.readUInt32LE(24);
    const sampleCount = (bytes.length - 44) / 4;
    const frames = sampleCount / 4;
    if (sourceRate < 8000 || sourceRate > 96000 || frames === 0 || frames / sourceRate > 60) {
      throw new Error(`${file}: unsupported rate or duration (maximum 60 seconds)`);
    }
    for (let i = 44; i < bytes.length; i += 4) {
      if (!Number.isFinite(bytes.readFloatLE(i))) throw new Error(`${file}: non-finite sample`);
    }
    const rendered = await page.evaluate(async ({ encoded, sourceRate, frames, rate }) => {
      const { buildStageGraph, applyStageToGraph } = await import('/src/lib/audioEngine.ts');
      const { CABIN_SPEC } = await import('/src/lib/cabin.ts');
      const binary = atob(encoded);
      const data = new DataView(Uint8Array.from(binary, (c) => c.charCodeAt(0)).buffer);
      // Include delayed arrivals and the entire room tail after the source ends.
      const tailS = CABIN_SPEC.impulse.durationS + 0.1;
      const outputs = {};
      for (const stage of ['raw', 'cockpit']) {
        const context = new OfflineAudioContext(2, Math.ceil((frames / sourceRate + tailS) * rate), rate);
        const buffer = context.createBuffer(4, frames, sourceRate);
        for (let path = 0; path < 4; path++) {
          const channel = buffer.getChannelData(path);
          for (let frame = 0; frame < frames; frame++) channel[frame] = data.getFloat32((frame * 4 + path) * 4, true);
        }
        const source = context.createBufferSource();
        source.buffer = buffer;
        source.channelInterpretation = 'discrete';
        const graph = buildStageGraph(context);
        applyStageToGraph(graph, stage, 0, 0);
        source.connect(graph.input);
        graph.output.connect(context.destination);
        source.start();
        const audio = await context.startRendering();
        const interleaved = new Float32Array(audio.length * 2);
        for (let i = 0; i < audio.length; i++) {
          interleaved[i * 2] = audio.getChannelData(0)[i];
          interleaved[i * 2 + 1] = audio.getChannelData(1)[i];
        }
        let peak = 0;
        for (const value of interleaved) peak = Math.max(peak, Math.abs(value));
        outputs[stage] = { samples: interleaved, peak };
      }
      // One shared trim for the comparison pair; never normalize the stages independently.
      const gain = Math.min(1, 0.98 / Math.max(outputs.raw.peak, outputs.cockpit.peak, 1e-12));
      for (const output of Object.values(outputs)) {
        let energy = 0;
        for (let i = 0; i < output.samples.length; i++) {
          output.samples[i] *= gain;
          energy += output.samples[i] ** 2;
        }
        output.rms = Math.sqrt(energy / output.samples.length);
        const bytes = new Uint8Array(output.samples.buffer);
        let binary = '';
        for (let i = 0; i < bytes.length; i += 8192) binary += String.fromCharCode(...bytes.subarray(i, i + 8192));
        output.encoded = btoa(binary);
        delete output.samples;
      }
      return { outputs, gain, tailS, cabinSpec: CABIN_SPEC };
    }, { encoded: bytes.subarray(44).toString('base64'), sourceRate, frames, rate });
    const tracks = {};
    for (const [stage, output] of Object.entries(rendered.outputs)) {
      const samples = Buffer.from(output.encoded, 'base64');
      const header = Buffer.from(bytes.subarray(0, 44));
      header.writeUInt32LE(samples.length + 36, 4);
      header.writeUInt16LE(2, 22);
      header.writeUInt32LE(rate, 24);
      header.writeUInt32LE(rate * 8, 28);
      header.writeUInt16LE(8, 32);
      header.writeUInt32LE(samples.length, 40);
      const name = `${stage}-${rate}.wav`;
      await writeFile(join(dirname(file), name), Buffer.concat([header, samples]));
      tracks[stage] = { file: name, peakBeforeTrim: output.peak, rms: output.rms };
    }
    const metadata = {
      source: 'paths.wav', sourceSha256: createHash('sha256').update(bytes).digest('hex'),
      sourceRate, outputRate: rate, channels: 2, sourceFrames: frames,
      tailS: rendered.tailS, commonGain: rendered.gain,
      browserVersion: browser.version(),
      graphSha256: createHash('sha256').update(await readFile(new URL('../src/lib/audioEngine.ts', import.meta.url))).digest('hex'),
      cabinModuleSha256: createHash('sha256').update(await readFile(new URL('../src/lib/cabin.ts', import.meta.url))).digest('hex'),
      cabinSpec: rendered.cabinSpec,
      transfer: 'buildStageGraph; browser resampling; excludes live worklet buffering/drift and master volume',
      tracks,
    };
    await writeFile(join(dirname(file), `render-${rate}.json`), JSON.stringify(metadata, null, 2));
    console.log(`${dirname(file)}: raw and cockpit at ${rate} Hz; common gain ${rendered.gain.toFixed(4)}`);
  }
} finally {
  await browser?.close();
  await server.close();
}
