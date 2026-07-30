#!/usr/bin/env node
/**
 * Generates the CLI reference from the built `lazydap` binary.
 *
 * It runs `lazydap --help`, reads the command list out of it, then runs
 * `lazydap <command> --help` for each one and again for any sub-subcommands.
 * Nothing here is transcribed by hand, so a flag that changes in the clap
 * definitions changes on the website at the next build.
 *
 * Output: src/content/docs/reference/cli/<command>.md, plus index.md.
 *
 * Options that appear on every single command (`--instance`, `--format`,
 * `--help`) are documented once on the index and left off the per-command
 * tables, because repeating the five-line `--format` value list on 21 pages
 * buries the flag the reader came for.
 *
 * Binary resolution: $LAZYDAP_BIN, else whichever of target/release/lazydap and
 * target/debug/lazydap was built most recently. Build one first —
 * `cargo build --bin lazydap`.
 *
 * This does NOT run as part of `npm run build`: the site has to build on a
 * machine with no Rust toolchain (a clean clone, or a Vercel deploy). The
 * generated pages are committed instead, and `npm run check:generated`
 * regenerates and asks git whether anything moved. CI runs that with
 * LAZYDAP_BIN pointing at a binary it just built.
 */

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const SITE_ROOT = resolve(__dirname, '..');
const REPO_ROOT = resolve(SITE_ROOT, '..');
const OUT_DIR = join(SITE_ROOT, 'src', 'content', 'docs', 'reference', 'cli');

const REPO_URL = 'https://github.com/planetaryescape/lazydap';

function resolveBinary() {
  if (process.env.LAZYDAP_BIN) {
    if (existsSync(process.env.LAZYDAP_BIN)) return process.env.LAZYDAP_BIN;
    console.error(`generate-cli-reference: LAZYDAP_BIN is set but missing: ${process.env.LAZYDAP_BIN}`);
    process.exit(1);
  }

  // Newest wins rather than release-then-debug. Preferring release would let a
  // months-old release binary answer --help after a clap change in a debug
  // build, and the freshness check would pass against the wrong CLI.
  const built = [
    join(REPO_ROOT, 'target', 'release', 'lazydap'),
    join(REPO_ROOT, 'target', 'debug', 'lazydap'),
  ]
    .filter((path) => existsSync(path))
    .map((path) => ({ path, mtime: statSync(path).mtimeMs }))
    .sort((a, b) => b.mtime - a.mtime);

  if (built.length > 0) return built[0].path;

  console.error(
    'generate-cli-reference: no lazydap binary found.\n' +
      `  Looked in ${join(REPO_ROOT, 'target')} for release/lazydap and debug/lazydap.\n` +
      '  Build one with `cargo build --bin lazydap`, or set LAZYDAP_BIN.',
  );
  process.exit(1);
}

const BIN = resolveBinary();

/** A binary that hangs must fail the build, not wedge it. Quirk 5 makes this real. */
const HELP_TIMEOUT_MS = 10_000;

function help(args) {
  const spelled = ['lazydap', ...args, '--help'].join(' ');
  try {
    return execFileSync(BIN, [...args, '--help'], {
      encoding: 'utf8',
      timeout: HELP_TIMEOUT_MS,
      maxBuffer: 8 * 1024 * 1024,
      // Long help wraps to the terminal width when there is one. Pin it so the
      // generated Markdown does not depend on whose terminal ran the build.
      env: { ...process.env, COLUMNS: '100', NO_COLOR: '1' },
    });
  } catch (error) {
    if (error.code === 'ETIMEDOUT' || error.signal === 'SIGTERM') {
      console.error(
        `generate-cli-reference: \`${spelled}\` did not answer within ${HELP_TIMEOUT_MS / 1000}s.\n` +
          `  Binary: ${BIN}\n` +
          '  A codelldb install wedged by a macOS update hangs like this; so does a debug build\n' +
          '  waiting on something. Run the command by hand to see which.',
      );
    } else {
      console.error(`generate-cli-reference: \`${spelled}\` failed.\n  ${error.message}`);
    }
    process.exit(1);
  }
}

/**
 * Splits clap's long `--help` into a description, a usage line, and the
 * `Arguments:` / `Options:` / `Commands:` sections.
 */
function parseHelp(text) {
  const lines = text.split('\n');
  const description = [];
  let usage = '';
  let i = 0;

  while (i < lines.length && !lines[i].startsWith('Usage:')) {
    description.push(lines[i]);
    i++;
  }
  if (i < lines.length) {
    usage = lines[i].replace(/^Usage:\s*/, '').trim();
    i++;
  }

  const sections = {};
  let current = null;
  let buffer = [];
  for (; i < lines.length; i++) {
    const header = lines[i].match(/^([A-Z][A-Za-z ]*):\s*$/);
    if (header) {
      if (current) sections[current] = buffer;
      current = header[1];
      buffer = [];
    } else {
      buffer.push(lines[i]);
    }
  }
  if (current) sections[current] = buffer;

  return { description: description.join('\n').trim(), usage, sections };
}

/**
 * Reads an `Arguments:` or `Options:` block. In clap's long help the token sits
 * at an indent below 10 and its prose is wrapped at an indent of 10 or more.
 */
function parseEntries(lines) {
  const entries = [];
  let current = null;

  for (const raw of lines) {
    if (!raw.trim()) continue;
    const indent = raw.length - raw.trimStart().length;
    const trimmed = raw.trim();

    if (indent < 10 && /^(-|<|\[|[A-Z_]+$)/.test(trimmed)) {
      if (current) entries.push(current);
      current = { token: trimmed, body: [] };
    } else if (current) {
      current.body.push(trimmed);
    }
  }
  if (current) entries.push(current);

  return entries.map((entry) => ({
    token: entry.token,
    description: flattenPossibleValues(entry.body.join(' ').replace(/\s+/g, ' ').trim()),
  }));
}

/**
 * clap renders enum flags as a `Possible values:` block followed by one
 * `- name: prose` line per variant. That does not fit a table cell, so the
 * variant names get folded into a trailing sentence and the per-variant prose
 * is dropped — the index page carries the long form for the flags that have one.
 */
function flattenPossibleValues(text) {
  const marker = text.indexOf('Possible values:');
  if (marker === -1) return text;

  const head = text.slice(0, marker).trim();
  const values = [...text.slice(marker).matchAll(/-\s+([a-z][\w-]*):/g)].map((m) => m[1]);
  if (values.length === 0) return head;
  const stop = /[.!?]$/.test(head) ? '' : '.';
  return `${head}${stop} One of ${values.map((v) => `\`${v}\``).join(', ')}.`.trim();
}

function parseCommands(lines) {
  const commands = [];
  for (const line of lines) {
    const match = line.match(/^\s{2,8}([\w-]+)\s{2,}(.*)$/);
    if (match) {
      // `[aliases: c]` is real information but belongs beside the name, not
      // trailing the summary.
      const aliases = match[2].match(/\[aliases?:\s*([^\]]+)\]/);
      commands.push({
        name: match[1],
        summary: match[2].replace(/\s*\[aliases?:[^\]]+\]\s*$/, '').trim(),
        aliases: aliases ? aliases[1].split(',').map((a) => a.trim()) : [],
      });
    }
  }
  return commands;
}

function escapeCell(text) {
  return text.replace(/\|/g, '\\|');
}

function table(header, rows) {
  if (rows.length === 0) return '';
  let md = `| ${header.join(' | ')} |\n| ${header.map(() => '---').join(' | ')} |\n`;
  for (const row of rows) md += `| ${row.map(escapeCell).join(' | ')} |\n`;
  return `${md}\n`;
}

function renderCommandBody(parsed, universal, headingLevel) {
  const h = '#'.repeat(headingLevel);
  let md = '';

  if (parsed.description) md += `${parsed.description}\n\n`;
  if (parsed.usage) md += `\`\`\`text\n${parsed.usage}\n\`\`\`\n\n`;

  const args = parseEntries(parsed.sections.Arguments ?? []);
  if (args.length > 0) {
    md += `${h} Arguments\n\n`;
    md += table(
      ['Argument', 'Description'],
      args.map((a) => [`\`${a.token}\``, a.description || '—']),
    );
  }

  const options = parseEntries(parsed.sections.Options ?? []).filter(
    (o) => !universal.has(o.token),
  );
  if (options.length > 0) {
    md += `${h} Options\n\n`;
    md += table(
      ['Flag', 'Description'],
      options.map((o) => [`\`${o.token}\``, o.description || '—']),
    );
    md += `Plus the [options every command takes](/reference/cli/#options-every-command-takes).\n\n`;
  }

  return md;
}

function main() {
  const rootParsed = parseHelp(help([]));
  const rootCommands = parseCommands(rootParsed.sections.Commands ?? []).filter(
    (c) => c.name !== 'help',
  );

  if (rootCommands.length === 0) {
    console.error('generate-cli-reference: `lazydap --help` listed no commands.');
    process.exit(1);
  }

  // Collect every command's parsed help first, so the universal-option set can
  // be computed by intersection rather than hardcoded.
  const parsedByCommand = new Map();
  const subcommandsByCommand = new Map();

  for (const command of rootCommands) {
    const parsed = parseHelp(help([command.name]));
    parsedByCommand.set(command.name, parsed);

    const subs = parseCommands(parsed.sections.Commands ?? []).filter((c) => c.name !== 'help');
    if (subs.length > 0) {
      subcommandsByCommand.set(
        command.name,
        subs.map((sub) => ({ ...sub, parsed: parseHelp(help([command.name, sub.name])) })),
      );
    }
  }

  let universal = null;
  for (const parsed of parsedByCommand.values()) {
    const tokens = new Set(parseEntries(parsed.sections.Options ?? []).map((o) => o.token));
    universal = universal === null ? tokens : new Set([...universal].filter((t) => tokens.has(t)));
  }
  universal ??= new Set();

  const universalEntries = parseEntries(
    parsedByCommand.get(rootCommands[0].name).sections.Options ?? [],
  ).filter((o) => universal.has(o.token));

  mkdirSync(OUT_DIR, { recursive: true });
  for (const existing of readdirSync(OUT_DIR)) {
    rmSync(join(OUT_DIR, existing), { recursive: true, force: true });
  }

  for (const command of rootCommands) {
    const parsed = parsedByCommand.get(command.name);
    const subs = subcommandsByCommand.get(command.name) ?? [];

    const summary = command.summary || `lazydap ${command.name}`;
    let md = `---\ntitle: "lazydap ${command.name}"\ndescription: ${JSON.stringify(summary)}\n---\n\n`;
    md += `:::note[Generated page]\nFrom \`lazydap ${command.name} --help\`. To change it, change the clap definition in [\`crates/daemon/src/cli.rs\`](${REPO_URL}/blob/main/crates/daemon/src/cli.rs), then run \`npm run generate\` in \`site/\` and commit the result. CI fails if this page and the binary disagree.\n:::\n\n`;

    if (command.aliases.length > 0) {
      md += `Also spelled ${command.aliases.map((a) => `\`lazydap ${a}\``).join(' or ')}.\n\n`;
    }

    md += renderCommandBody(parsed, universal, 2);

    for (const sub of subs) {
      md += `## \`lazydap ${command.name} ${sub.name}\`\n\n`;
      md += renderCommandBody(sub.parsed, universal, 3);
    }

    md += `## See also\n\n`;
    md += `- [CLI overview](/reference/cli/) — every command in one list\n`;
    md += `- [JSON output](/reference/json-output/) — the shape of what comes back\n`;
    md += `- [Errors and exit codes](/reference/errors/) — what to do when it fails\n`;

    writeFileSync(join(OUT_DIR, `${command.name}.md`), md);
  }

  // ---- index ----
  let index = `---\ntitle: CLI reference\ndescription: Every lazydap command, generated from the binary's own help.\n---\n\n`;
  index += `:::note[Generated page]\nFrom \`lazydap --help\` and one \`lazydap <command> --help\` per command. CI regenerates it and fails if anything drifts, so a flag listed here exists in the binary that produced it.\n:::\n\n`;
  index += `lazydap is one binary with a subcommand per debugger operation. Run it with no arguments on a terminal and you get [the TUI](/getting-started/tui/); run it anywhere else and you get this help.\n\n`;
  index += `\`\`\`text\n${rootParsed.usage}\n\`\`\`\n\n`;

  index += `## Commands\n\n`;
  index += table(
    ['Command', 'Does'],
    rootCommands.map((c) => [
      `[\`lazydap ${c.name}\`](/reference/cli/${c.name}/)${
        c.aliases.length ? ` <br/>\`${c.aliases.join('`, `')}\`` : ''
      }`,
      c.summary || '—',
    ]),
  );

  index += `## Options every command takes\n\n`;
  index += table(
    ['Flag', 'Description'],
    universalEntries.map((o) => [`\`${o.token}\``, o.description || '—']),
  );

  index += `\`--format\` decides how a command answers. \`table\` is for reading and its layout is not a contract; \`json\` is [the contract](/reference/json-output/). With no \`--format\`, lazydap picks \`table\` on a terminal and \`json\` everywhere else, so a pipeline gets JSON without asking.\n\n`;

  index += `## Commands that move the program\n\n`;
  index += `\`continue\`, \`step\`, \`step-in\`, \`step-out\` and \`pause\` also take \`--wait\` and \`--timeout\`. Without \`--wait\` they return as soon as the debugger accepts the request, which is what a live UI wants and almost never what a script wants. See [the \`--wait\` contract](/guides/wait/).\n\n`;
  index += `\`launch\` does **not** take \`--wait\` — it answers with its own shape once the configuration phase is done. Pass \`--stop-on-entry\` to hold the program still, then \`continue --wait\` to move it.\n\n`;

  index += `## See also\n\n`;
  index += `- [Quickstart](/getting-started/quickstart/) — the commands in order, against a real program\n`;
  index += `- [JSON output](/reference/json-output/) — field-by-field schemas\n`;
  index += `- [Errors and exit codes](/reference/errors/) — every error name lazydap emits\n`;

  writeFileSync(join(OUT_DIR, 'index.md'), index);

  console.log(
    `[cli-reference] ${rootCommands.length} command pages from ${BIN} ` +
      `(${universal.size} shared options hoisted to the index)`,
  );
}

main();
