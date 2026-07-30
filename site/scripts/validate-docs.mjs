#!/usr/bin/env node
/**
 * Pre-build checks on the docs sources.
 *
 * 1. Internal links resolve to a page that exists.
 * 2. No two files claim the same Starlight slug.
 * 3. Every page has a title and a description.
 * 4. Claims that were once wrong on this site, or are wrong in the blueprint
 *    docs a writer might copy from, stay wrong-proofed.
 *
 * Run by `npm run build` before Astro sees anything. Exit 1 on any failure.
 */

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = fileURLToPath(new URL('.', import.meta.url));
const SITE_ROOT = resolve(__dirname, '..');
const DOCS_ROOT = join(SITE_ROOT, 'src', 'content', 'docs');
const PUBLIC_ROOT = join(SITE_ROOT, 'public');

/**
 * Each of these was either wrong on this site once, or is wrong in a repo doc
 * that is otherwise a good source to write from. The blueprint predates the
 * code in several places; these rules stop it being copied in.
 */
const banned = [
  {
    re: /"state":\s*"(Paused|Exited|Terminated|Timeout|AdapterDied)"/,
    message:
      'wire values are snake_case ("paused", "adapter_died"); the CamelCase in docs/blueprint/10-async-to-sync.md is stale',
  },
  {
    re: /"reason":\s*\{\s*"Breakpoint"/,
    message: '`reason` serialises as a bare string, not a tagged object (blueprint examples are stale)',
  },
  {
    re: /adapter-codelldb|adapter-fake|adapter-debugpy|adapter-js-debug/,
    message:
      'there is no adapter-* crate; codelldb lives in crates/daemon/src/adapter/ (ARCHITECTURE.md is stale here)',
  },
  // Commands that do not exist are caught by the generated CLI reference being
  // the only place commands are enumerated, so there is no rule for them here:
  // pages legitimately name `attach` and `restart` when listing what is unbuilt.
  {
    re: /sixth IPC bucket/,
    message: 'there are four IPC buckets, not five',
  },
  {
    re: /\|\s*xargs\s+-r\b/,
    message: 'GNU-only `xargs -r`; macOS xargs has no such flag',
  },
  {
    re: /\bseamless|\beffortless|\brobust\b|\bpowerful\b|\bcomprehensive\b|\bleverage[sd]?\b/i,
    message: 'banned marketing vocabulary',
  },
];

function* walk(dir) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) yield* walk(path);
    else if (/\.(md|mdx)$/.test(path)) yield path;
  }
}

/** `getting-started/install.md` -> `/getting-started/install/` */
function routeFor(file) {
  const rel = relative(DOCS_ROOT, file).split(sep).join('/');
  const slug = rel.replace(/\.(md|mdx)$/, '').replace(/(^|\/)index$/, '');
  return slug === '' ? '/' : `/${slug}/`;
}

let failed = 0;
const fail = (file, message) => {
  console.error(`[validate] ${relative(SITE_ROOT, file)}: ${message}`);
  failed++;
};

const files = [...walk(DOCS_ROOT)];
const routes = new Map();

for (const file of files) {
  const route = routeFor(file);
  if (routes.has(route)) {
    fail(file, `duplicate route ${route}, already claimed by ${relative(SITE_ROOT, routes.get(route))}`);
  }
  routes.set(route, file);
}

for (const file of files) {
  const text = readFileSync(file, 'utf8');

  if (!/^---\n[\s\S]*?\n---/.test(text)) {
    fail(file, 'no frontmatter');
    continue;
  }
  const frontmatter = text.slice(4, text.indexOf('\n---', 4));
  if (!/^title:/m.test(frontmatter)) fail(file, 'frontmatter has no title');
  if (!/^description:/m.test(frontmatter)) fail(file, 'frontmatter has no description');

  for (const rule of banned) {
    const match = text.match(rule.re);
    if (match) fail(file, `${rule.message} (matched "${match[0]}")`);
  }

  // Internal links. Markdown link targets that start with `/` must resolve to a
  // page route or a file in public/. Anchors are checked for the page part only.
  for (const [, target] of text.matchAll(/\]\((\/[^)\s"']*)\)/g)) {
    const [path, hash] = target.split('#');
    if (path === '') continue; // same-page anchor
    const normalised = path.endsWith('/') ? path : `${path}/`;

    if (routes.has(normalised)) continue;
    if (existsSync(join(PUBLIC_ROOT, path.replace(/^\//, '')))) continue;

    fail(file, `dead internal link ${target}${hash ? '' : ''}`);
  }
}

if (failed > 0) {
  console.error(`[validate] ${failed} problem${failed === 1 ? '' : 's'}`);
  process.exit(1);
}

console.log(`[validate] ok — ${files.length} pages, ${routes.size} routes, no dead internal links`);
