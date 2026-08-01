#!/usr/bin/env node
/**
 * Generates the adapter quirk references from `docs/reference/*-quirks.md`.
 *
 * Those three files are the source of truth: they are written while the quirk is
 * being found, next to the code that works around it. The site used to carry a
 * hand-maintained retelling of the codelldb one, which drifted to 8 entries
 * against the repo's 21 while debugpy and delve had no page at all. Generating
 * removes the copy, so the site cannot silently fall behind again.
 *
 * Output: src/content/docs/reference/<adapter>-quirks.md, committed.
 *
 * Two things have to change on the way through:
 *
 *   1. Frontmatter. The repo files open with an H1; Starlight renders the
 *      frontmatter title as the H1, so the original is dropped to avoid two.
 *   2. Links. Repo-relative Markdown links mean nothing on a website. Sibling
 *      quirk files become site routes; everything else — the blueprint, the
 *      milestone tasks, CONTRIBUTING, the book — has no site page, so it points
 *      at GitHub rather than being dropped. Dropping would lose the reference
 *      entirely, and every one of these targets exists in the public repo.
 *
 * This does NOT run as part of `npm run build`, matching generate-cli-reference:
 * the generated pages are committed and `npm run check:generated` regenerates
 * and asks git whether anything moved. Unlike the CLI generator it needs no
 * lazydap binary — only the repo checkout — so it is safe to run anywhere.
 */

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const SITE_ROOT = resolve(__dirname, '..');
const REPO_ROOT = resolve(SITE_ROOT, '..');
const SRC_DIR = join(REPO_ROOT, 'docs', 'reference');
const OUT_DIR = join(SITE_ROOT, 'src', 'content', 'docs', 'reference');

const REPO_URL = 'https://github.com/planetaryescape/lazydap';
const BLOB = `${REPO_URL}/blob/main`;

/**
 * Sibling documents that DO have a site page. Anything not in here and not
 * absolute gets a GitHub link, because the site has no blueprint, no issues
 * directory and no book.
 */
const SITE_ROUTES = new Map([
  ['docs/reference/codelldb-quirks.md', '/reference/codelldb-quirks/'],
  ['docs/reference/debugpy-quirks.md', '/reference/debugpy-quirks/'],
  ['docs/reference/delve-quirks.md', '/reference/delve-quirks/'],
]);

const SOURCES = [
  {
    file: 'codelldb-quirks.md',
    title: 'codelldb quirks',
    description: (n) =>
      `All ${n} codelldb behaviours that have cost this project time, each with its cause and its fix.`,
  },
  {
    file: 'debugpy-quirks.md',
    title: 'debugpy quirks',
    description: (n) =>
      `${n} places debugpy behaves differently from the other adapters, captured off the wire.`,
  },
  {
    file: 'delve-quirks.md',
    title: 'delve quirks',
    description: (n) =>
      `${n} delve behaviours lazydap had to be told about, found by reading the wire rather than the documentation.`,
  },
];

/** `docs/reference` + `../blueprint/15-decision-log.md` -> `docs/blueprint/15-decision-log.md` */
function repoPathFor(target) {
  const segments = 'docs/reference'.split('/');
  for (const part of target.split('/')) {
    if (part === '.' || part === '') continue;
    if (part === '..') segments.pop();
    else segments.push(part);
  }
  return segments.join('/');
}

/**
 * Rewrites one Markdown link target. Absolute URLs and same-page anchors are
 * already correct and pass through untouched.
 */
function rewriteTarget(target) {
  if (/^(https?:|mailto:|#|\/)/.test(target)) return target;

  const [path, hash] = target.split('#');
  const suffix = hash ? `#${hash}` : '';
  const repoPath = repoPathFor(path);

  const route = SITE_ROUTES.get(repoPath);
  if (route) return `${route}${suffix}`;

  return `${BLOB}/${repoPath}${suffix}`;
}

function rewriteLinks(markdown) {
  const rewritten = new Map();

  // Only Markdown inline links. Bare paths in prose and in code spans are left
  // alone: `crates/daemon/src/adapter/codelldb.rs` reads as a path, not a link,
  // and turning it into one would be a change of meaning rather than of form.
  const out = markdown.replace(/\]\(([^)\s]+)\)/g, (whole, target) => {
    const next = rewriteTarget(target);
    if (next !== target) rewritten.set(target, next);
    return `](${next})`;
  });

  return { out, rewritten };
}

function main() {
  let totalQuirks = 0;
  let totalRewrites = 0;

  for (const source of SOURCES) {
    const raw = readFileSync(join(SRC_DIR, source.file), 'utf8');

    // Drop the leading H1 and any blank lines after it; Starlight supplies one
    // from the frontmatter title and two H1s render as a doubled heading.
    const body = raw.replace(/^#\s+.*\n+/, '');

    const quirkCount = [...body.matchAll(/^##\s+\d+\./gm)].length;
    if (quirkCount === 0) {
      console.error(
        `generate-quirks: ${source.file} has no \`## N.\` quirk headings.\n` +
          '  Either the file changed shape or the wrong file was read.',
      );
      process.exit(1);
    }
    totalQuirks += quirkCount;

    const { out: linked, rewritten } = rewriteLinks(body);
    totalRewrites += rewritten.size;

    const description = source.description(quirkCount);
    let md = `---\ntitle: ${JSON.stringify(source.title)}\ndescription: ${JSON.stringify(description)}\n---\n\n`;
    md += `:::note[Generated page]\n`;
    md += `From [\`docs/reference/${source.file}\`](${BLOB}/docs/reference/${source.file}) in the repository, which is where these are written as they are found. `;
    md += `To change this page, change that file, then run \`npm run generate\` in \`site/\` and commit the result. CI fails if the two disagree.\n`;
    md += `:::\n\n`;
    md += linked.trimEnd();

    // The repo files cross-reference each other but know nothing about the
    // site's guides or its symptom-organised troubleshooting page, so the
    // site-native links are appended here rather than written upstream.
    const others = SOURCES.filter((s) => s.file !== source.file);
    md += `\n\n## See also\n\n`;
    md += `- [Write one script for four languages](/guides/adapters/) — what differs between the three adapters, side by side\n`;
    for (const other of others) {
      md += `- [${other.title}](/reference/${other.file.replace(/\.md$/, '')}/) — the same treatment for ${other.title.replace(' quirks', '')}\n`;
    }
    md += `- [Troubleshooting](/troubleshooting/) — the same ground, organised by symptom\n`;

    writeFileSync(join(OUT_DIR, source.file), md);
    console.log(`[quirks] ${source.file}: ${quirkCount} quirks, ${rewritten.size} links rewritten`);
  }

  console.log(`[quirks] ${SOURCES.length} pages, ${totalQuirks} quirks, ${totalRewrites} links rewritten`);
}

main();
