// @ts-check
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

const SITE_ROOT = dirname(fileURLToPath(import.meta.url));
const SIDEBAR_PATH = join(SITE_ROOT, 'src', 'data', 'sidebar.json');
const sidebar = existsSync(SIDEBAR_PATH)
  ? JSON.parse(readFileSync(SIDEBAR_PATH, 'utf8'))
  : [];

const OG_IMAGE_URL = 'https://asamarts.github.io/witforge/og-card.png';

export default defineConfig({
  site: 'https://asamarts.github.io',
  base: '/witforge',
  trailingSlash: 'always',
  integrations: [
    starlight({
      title: 'witforge',
      description:
        'WIT-first contract compiler for deterministic Rust, Protobuf, JSON Schema, checked glue, Markdown docs, and version adapters.',
      favicon: '/favicon.svg',
      disable404Route: true,
      head: [
        {
          tag: 'meta',
          attrs: { name: 'theme-color', content: '#ffffff' },
        },
        {
          tag: 'meta',
          attrs: { property: 'og:image', content: OG_IMAGE_URL },
        },
        {
          tag: 'meta',
          attrs: { property: 'og:image:width', content: '1200' },
        },
        {
          tag: 'meta',
          attrs: { property: 'og:image:height', content: '630' },
        },
        {
          tag: 'meta',
          attrs: { name: 'twitter:card', content: 'summary_large_image' },
        },
        {
          tag: 'meta',
          attrs: { name: 'twitter:image', content: OG_IMAGE_URL },
        },
      ],
      expressiveCode: {
        themes: ['github-light', 'github-dark'],
        useStarlightUiThemeColors: true,
      },
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/asamarts/witforge',
        },
      ],
      components: {
        Header: './src/components/starlight/Header.astro',
        Footer: './src/components/starlight/Footer.astro',
      },
      customCss: [
        '@fontsource-variable/plus-jakarta-sans/wght.css',
        '@fontsource-variable/inter/wght.css',
        '@fontsource-variable/inter/wght-italic.css',
        '@fontsource/jetbrains-mono/400.css',
        '@fontsource/jetbrains-mono/600.css',
        './src/styles/tokens.css',
        './src/styles/custom.css',
      ],
      sidebar,
    }),
  ],
});
