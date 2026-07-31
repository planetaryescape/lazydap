import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// The domain is not decided yet (M20 leaves it to the user). `site` still has to
// be set — Starlight derives canonical URLs, og:url and the sitemap from it — so
// this is a placeholder that a deploy can override with SITE_URL without a code
// change. Change both this default and the og:image URLs below when it lands.
const SITE_URL = process.env.SITE_URL ?? 'https://lazydap.sh';

export default defineConfig({
  site: SITE_URL,
  integrations: [
    starlight({
      title: 'lazydap',
      description:
        'Debug a C, C++ or Rust program from the shell. Set a breakpoint, run to it, read the JSON that comes back. A daemon holds the session so every command can be its own process.',
      disable404Route: true,
      social: [
        { icon: 'github', label: 'GitHub', href: 'https://github.com/planetaryescape/lazydap' },
      ],
      customCss: ['./src/styles/custom.css'],
      expressiveCode: {
        // Overflowing code blocks get an unnamed scroll region, which is a
        // keyboard trap on phones. Wrapping avoids it; the copy button still
        // copies the unwrapped original.
        defaultProps: {
          wrap: true,
          preserveIndent: false,
        },
      },
      components: {
        // Starlight's Banner only renders when a page sets `banner` frontmatter.
        // The pre-release notice belongs on every page, so it is an override
        // rather than 20 copies of the same frontmatter. Delete this entry when
        // v0.1.0 is tagged.
        Banner: './src/components/Banner.astro',
      },
      head: [
        { tag: 'meta', attrs: { name: 'theme-color', content: '#0d1117' } },
        // Starlight already emits canonical, og:title/type/url/locale/description/
        // site_name and twitter:card from `site` plus page frontmatter. These are
        // the ones it does not. og:image must be absolute.
        // TODO: public/og.png does not exist yet. Until it does, social cards
        // fall back to no image, which degrades gracefully. Ship a 1200x630 PNG
        // at that path — no other change is needed here.
        // Do not add twitter:title or twitter:description: a single site-wide
        // value overrides Starlight's per-page og:* fallback and labels every
        // page with the homepage's title.
        { tag: 'meta', attrs: { property: 'og:image', content: `${SITE_URL}/og.png` } },
        { tag: 'meta', attrs: { property: 'og:image:width', content: '1200' } },
        { tag: 'meta', attrs: { property: 'og:image:height', content: '630' } },
        { tag: 'meta', attrs: { property: 'og:image:type', content: 'image/png' } },
        {
          tag: 'meta',
          attrs: {
            property: 'og:image:alt',
            content: 'lazydap: debug a C, C++ or Rust program from the shell.',
          },
        },
        { tag: 'meta', attrs: { name: 'twitter:image', content: `${SITE_URL}/og.png` } },
        { tag: 'meta', attrs: { name: 'author', content: 'planetaryescape' } },
        {
          tag: 'meta',
          attrs: {
            name: 'keywords',
            content:
              'debugger, CLI debugger, terminal debugger, Debug Adapter Protocol, DAP, codelldb, LLDB, agent debugging, AI agent tools, Rust debugger, C debugger, scriptable debugger, TUI debugger',
          },
        },
      ],
      sidebar: [
        {
          label: 'Start Here',
          items: [
            { label: 'Install', slug: 'getting-started/install' },
            { label: 'Quickstart', slug: 'getting-started/quickstart' },
            { label: 'The TUI', slug: 'getting-started/tui' },
          ],
        },
        {
          label: 'Guides',
          items: [
            { label: 'Recipes', slug: 'guides/recipes' },
            { label: 'Why lazydap', slug: 'guides/why-lazydap' },
            { label: 'Debug with an agent', slug: 'guides/agents' },
            { label: 'The --wait contract', slug: 'guides/wait' },
            { label: 'Breakpoints', slug: 'guides/breakpoints' },
            { label: 'Output formats and piping', slug: 'guides/output-formats' },
            { label: 'The daemon', slug: 'guides/daemon' },
            { label: 'Architecture', slug: 'guides/architecture' },
          ],
        },
        {
          label: 'Reference',
          items: [
            {
              label: 'CLI',
              items: [
                { label: 'Overview', slug: 'reference/cli' },
                {
                  label: 'Commands',
                  collapsed: true,
                  items: [{ autogenerate: { directory: 'reference/cli', collapsed: true } }],
                },
              ],
            },
            { label: 'JSON output', slug: 'reference/json-output' },
            { label: 'Errors and exit codes', slug: 'reference/errors' },
            { label: 'Protocol', slug: 'reference/protocol' },
            { label: 'codelldb quirks', slug: 'reference/codelldb-quirks' },
          ],
        },
        {
          label: 'Troubleshooting',
          items: [{ label: 'Troubleshooting', slug: 'troubleshooting' }],
        },
      ],
    }),
  ],
});
