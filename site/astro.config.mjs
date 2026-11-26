// @ts-check
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';
import starlight from '@astrojs/starlight';

const SITE_ROOT = dirname(fileURLToPath(import.meta.url));
const SIDEBAR_PATH = join(SITE_ROOT, 'src', 'data', 'sidebar.json');
const sidebar = existsSync(SIDEBAR_PATH)
  ? JSON.parse(readFileSync(SIDEBAR_PATH, 'utf8'))
  : [];

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
      expressiveCode: {
        themes: ['github-light', 'github-dark'],
      },
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/asamarts/witforge',
        },
      ],
      customCss: [
        '@fontsource-variable/plus-jakarta-sans/wght.css',
        '@fontsource-variable/inter/wght.css',
        '@fontsource/jetbrains-mono/400.css',
        '@fontsource/jetbrains-mono/600.css',
        './src/styles/tokens.css',
      ],
      sidebar,
    }),
    sitemap(),
  ],
});
