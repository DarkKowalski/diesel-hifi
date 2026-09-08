import { build, preview } from 'astro';

// Keep test servers attached to Playwright in agent environments too. The Astro
// CLI may daemonize automatically; its programmatic API keeps ownership explicit.
const [, , directory, port, base] = process.argv;
if (!['dist', 'dist-subpath'].includes(directory) || !port || !base) {
  throw new Error('Expected output directory, port, and base');
}
const config = {
  outDir: directory,
  base,
  server: { port: Number(port), host: '127.0.0.1' },
  logLevel: 'error',
};
await build(config);
const server = await preview(config);
for (const signal of ['SIGINT', 'SIGTERM']) {
  process.once(signal, async () => { await server.stop(); process.exit(0); });
}
