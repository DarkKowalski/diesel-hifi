#!/usr/bin/env node
/**
 * Static-build verification.
 *
 * SPEC section 10: "`web/dist` contains a complete static deployment with no
 * runtime external requests."
 *
 * This checks the artifact on disk: that the deployment is structurally
 * complete, and that nothing in it can *load* from off-build. It distinguishes
 * load positions (an `href`, a `fetch`, a CSS `url()`) from incidental strings
 * such as the documentation link embedded in Svelte's runtime error messages.
 * The Playwright suite proves the stronger runtime property — that a real
 * browser session issues zero external requests.
 *
 * Exits non-zero on the first failure.
 */

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';

const DIST = resolve(process.cwd(), process.argv[2] ?? 'web/dist');

/** Hosts that are always a red flag, wherever they appear. */
const FORBIDDEN_HOSTS =
  /(fonts\.(googleapis|gstatic)\.com|unpkg\.com|cdn\.jsdelivr\.net|cdnjs\.cloudflare\.com|\/\/cdn\.)/gi;

/** Constructs that would actually fetch from an absolute URL at runtime. */
const LOAD_CONSTRUCTS = [
  { pattern: /\bfetch\s*\(\s*["'`]https?:\/\//gi, what: 'a fetch() of an absolute URL' },
  { pattern: /\bimportScripts\s*\(\s*["'`]https?:\/\//gi, what: 'an importScripts() of an absolute URL' },
  { pattern: /\bnew\s+Worker\s*\(\s*["'`]https?:\/\//gi, what: 'a Worker built from an absolute URL' },
  { pattern: /\bimport\s*\(\s*["'`]https?:\/\//gi, what: 'a dynamic import of an absolute URL' },
  { pattern: /\bfrom\s*["'`]https?:\/\//gi, what: 'a static import from an absolute URL' },
  {
    pattern: /SharedArrayBuffer/g,
    what: 'a SharedArrayBuffer use (would require cross-origin isolation)',
  },
];

/** Absolute URLs that are known to be inert strings, not loads. */
const INERT_HOSTS = new Set(['svelte.dev', 'www.svelte.dev']);

const TEXT_EXTENSIONS = new Set(['.html', '.js', '.mjs', '.css', '.json', '.map', '.svg']);

const failures = [];
const notes = [];

const fail = (message) => failures.push(message);

function walk(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) out.push(...walk(full));
    else out.push(full);
  }
  return out;
}

let files;
try {
  files = walk(DIST);
} catch {
  console.error(`verify-dist: ${DIST} does not exist. Run \`pnpm build\` first.`);
  process.exit(1);
}

if (files.length === 0) fail(`${DIST} is empty`);

// --- structure ---------------------------------------------------------------

const relatives = files.map((f) => relative(DIST, f));

if (!relatives.includes('index.html')) fail('dist is missing index.html');

const wasmFiles = relatives.filter((f) => f.endsWith('.wasm'));
if (wasmFiles.length === 0) {
  fail('dist ships no .wasm binary; the simulation core is missing');
}

const jsFiles = relatives.filter((f) => f.endsWith('.js'));
if (jsFiles.length < 2) {
  fail('dist should contain at least an entry chunk and a separate worker chunk');
}

// The worker must be its own chunk, not inlined: the simulation has to run off
// the UI thread.
const workerChunk = jsFiles.find((f) => /worker/i.test(f));
if (workerChunk === undefined) {
  fail('dist contains no worker chunk; the simulation may not be running off the UI thread');
}

if (!relatives.some((f) => f.endsWith('.css'))) {
  fail('dist ships no stylesheet; styling must be local to the build');
}

// --- content -----------------------------------------------------------------

const inertHostsSeen = new Set();

for (const file of files) {
  const extension = file.slice(file.lastIndexOf('.'));
  if (!TEXT_EXTENSIONS.has(extension)) continue;

  const name = relative(DIST, file);
  const text = readFileSync(file, 'utf8');

  for (const match of text.matchAll(FORBIDDEN_HOSTS)) {
    fail(`${name} references a third-party host: ${match[0]}`);
  }

  for (const { pattern, what } of LOAD_CONSTRUCTS) {
    const matches = text.match(pattern);
    if (matches !== null) {
      fail(`${name} contains ${what}: ${[...new Set(matches)].slice(0, 3).join(', ')}`);
    }
  }

  // Markup and stylesheet URLs are always load positions.
  if (extension === '.html') {
    for (const match of text.matchAll(/(?:src|href)\s*=\s*"([^"]+)"/g)) {
      if (/^https?:|^\/\//i.test(match[1])) {
        fail(`${name} references an off-build asset: ${match[1]}`);
      }
    }
  }
  if (extension === '.css') {
    for (const match of text.matchAll(/url\(\s*['"]?(https?:\/\/[^)'"]+)/gi)) {
      fail(`${name} loads an external stylesheet asset: ${match[1]}`);
    }
    if (/@import\s+(?:url\()?['"]?https?:/i.test(text)) {
      fail(`${name} @imports an external stylesheet`);
    }
  }

  // Anything else absolute is an inert string; record it so it stays visible.
  for (const match of text.matchAll(/https?:\/\/([a-z0-9.-]+)/gi)) {
    const host = match[1].toLowerCase();
    if (host === 'localhost' || host === '127.0.0.1') continue;
    if (INERT_HOSTS.has(host)) {
      inertHostsSeen.add(host);
    } else {
      fail(`${name} contains an unrecognised absolute URL host: ${host}`);
    }
  }
}

// --- report ------------------------------------------------------------------

const totalBytes = files.reduce((sum, f) => sum + statSync(f).size, 0);
notes.push(`${files.length} files, ${(totalBytes / 1024).toFixed(0)} KiB total`);
notes.push(`wasm: ${wasmFiles.join(', ')}`);
if (workerChunk !== undefined) notes.push(`worker chunk: ${workerChunk}`);
if (inertHostsSeen.size > 0) {
  notes.push(
    `inert URL strings (not loaded, allowlisted): ${[...inertHostsSeen].sort().join(', ')}`,
  );
}

if (failures.length > 0) {
  console.error(`verify-dist: FAILED (${DIST})`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}

console.log(`verify-dist: OK (${DIST})`);
for (const note of notes) console.log(`  - ${note}`);
