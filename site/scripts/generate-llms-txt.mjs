#!/usr/bin/env node
/**
 * Post-build hook.
 *
 * 1. Concatenates every Markdown source under `src/content/docs` into
 *    `dist/llms-full.txt`, so a model can take the whole corpus in one fetch.
 * 2. Writes a `.md` sibling for every page, so `curl https://host/guides/wait.md`
 *    returns Markdown instead of minified HTML.
 *
 * The curated `llms.txt` index is hand-written at `public/llms.txt` and copied
 * into `dist/` by Astro. It is not regenerated here — a curated index is worth
 * more to a model than a second machine-ordered list.
 */

import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const SITE_ROOT = resolve(__dirname, '..');
const DOCS_ROOT = join(SITE_ROOT, 'src', 'content', 'docs');
const DIST_ROOT = join(SITE_ROOT, 'dist');
const SITE_URL = (process.env.SITE_URL ?? 'https://lazydap.sh').replace(/\/$/, '');

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full, out);
    else if (/\.(md|mdx)$/.test(entry)) out.push(full);
  }
  return out;
}

function splitFrontmatter(raw) {
  if (!raw.startsWith('---')) return { body: raw, title: null, description: null };
  const end = raw.indexOf('\n---', 3);
  if (end === -1) return { body: raw, title: null, description: null };
  const fm = raw.slice(3, end);
  const body = raw.slice(end + 4).replace(/^\n+/, '');
  const pick = (key) =>
    (fm.match(new RegExp(`^${key}:\\s*(?:"([^"]*)"|'([^']*)'|(.+))$`, 'm')) || [])
      .slice(1)
      .find(Boolean) ?? null;
  return { body, title: pick('title'), description: pick('description') };
}

function routeFor(file) {
  const rel = relative(DOCS_ROOT, file).replace(/\.(md|mdx)$/, '').replace(/\\/g, '/');
  return rel === 'index' ? '/' : `/${rel.replace(/\/index$/, '')}/`;
}

if (!existsSync(DOCS_ROOT)) {
  console.warn(`[llms] no docs at ${DOCS_ROOT}; skipping`);
  process.exit(0);
}
if (!existsSync(DIST_ROOT)) mkdirSync(DIST_ROOT, { recursive: true });

const files = walk(DOCS_ROOT).sort();

const corpus = [
  '# lazydap — full documentation',
  '',
  'Every page of the lazydap docs, concatenated at build time.',
  'For a curated index instead, fetch /llms.txt.',
  '',
];

for (const file of files) {
  const { body, title, description } = splitFrontmatter(readFileSync(file, 'utf8'));
  const route = routeFor(file);
  corpus.push('---', '', `# ${title ?? route}`, `URL: ${SITE_URL}${route}`);
  if (description) corpus.push(`> ${description}`);
  corpus.push('', body.trim(), '');
}

writeFileSync(join(DIST_ROOT, 'llms-full.txt'), corpus.join('\n'));

let siblings = 0;
for (const file of files) {
  const { body, title, description } = splitFrontmatter(readFileSync(file, 'utf8'));
  const rel = relative(DOCS_ROOT, file).replace(/\.(md|mdx)$/, '').replace(/\\/g, '/');
  const target = join(DIST_ROOT, `${rel}.md`);
  mkdirSync(dirname(target), { recursive: true });

  const out = [];
  if (title) out.push(`# ${title}`);
  if (description) out.push(`> ${description}`);
  if (out.length) out.push('');
  out.push(body.trim());
  writeFileSync(target, `${out.join('\n')}\n`);
  siblings++;
}

console.log(`[llms] llms-full.txt from ${files.length} pages, ${siblings} .md siblings`);
