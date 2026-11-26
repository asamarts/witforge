import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { toPosix } from './markdown-utils.mjs';

const SCRIPTS_DIR = dirname(fileURLToPath(import.meta.url));
const DEFAULT_SITE_ROOT = resolve(SCRIPTS_DIR, '..');
const DEFAULT_DIST_DIR = resolve(DEFAULT_SITE_ROOT, 'dist');
const DEFAULT_MANIFEST_PATH = resolve(SCRIPTS_DIR, 'redirects.json');

export const DEFAULT_SITE_ORIGIN = 'https://asamarts.github.io';
export const DEFAULT_BASE_PATH = '/witforge';
export const REDIRECT_MARKER = '<!-- witforge-legacy-redirect -->';

const RESERVED_SOURCES = new Set(['/index.html', '/404.html']);

/**
 * Escape HTML special characters for safe inclusion in text nodes and attribute values.
 */
export function escapeHtml(value) {
  return String(value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

/**
 * Serialize a string as an inline JS string literal safe inside `<script>` tags.
 */
export function escapeJsString(value) {
  return JSON.stringify(String(value))
    .replace(/</g, '\\u003c')
    .replace(/>/g, '\\u003e')
    .replace(/&/g, '\\u0026')
    .replace(/\u2028/g, '\\u2028')
    .replace(/\u2029/g, '\\u2029');
}

function hasUnsafePathSegments(route) {
  if (route.includes('\\') || route.includes('?') || route.includes('#')) {
    return true;
  }
  if (/[\u0000-\u001f\u007f\s]/.test(route)) {
    return true;
  }
  const parts = route.split('/').filter(Boolean);
  return parts.some((part) => part === '.' || part === '..');
}

/**
 * Validate a single redirect manifest entry.
 */
export function validateRedirectEntry(entry, index, { basePath = DEFAULT_BASE_PATH } = {}) {
  const normalizedBase = basePath.replace(/\/+$/, '');
  if (!entry || typeof entry !== 'object' || Array.isArray(entry)) {
    throw new Error(`redirects.json entry #${index + 1} must be an object`);
  }

  const { from, to, title } = entry;
  if (typeof from !== 'string' || !from.startsWith('/') || !from.endsWith('.html')) {
    throw new Error(
      `redirects.json entry #${index + 1} has invalid "from" (${JSON.stringify(from)}); expected leading "/" and ".html" suffix`,
    );
  }
  if (hasUnsafePathSegments(from) || from.includes('//')) {
    throw new Error(`redirects.json entry #${index + 1} has unsafe "from" path: ${from}`);
  }
  if (RESERVED_SOURCES.has(from.toLowerCase())) {
    throw new Error(`redirects.json entry #${index + 1} cannot overwrite reserved path: ${from}`);
  }

  const expectedPrefix = `${normalizedBase}/`;
  if (typeof to !== 'string' || !to.startsWith(expectedPrefix) || !to.endsWith('/')) {
    throw new Error(
      `redirects.json entry #${index + 1} has invalid "to" (${JSON.stringify(to)}); expected "${expectedPrefix}.../"`,
    );
  }
  if (hasUnsafePathSegments(to) || to.includes('//')) {
    throw new Error(`redirects.json entry #${index + 1} has unsafe "to" path: ${to}`);
  }

  if (title !== undefined && (typeof title !== 'string' || title.trim().length === 0)) {
    throw new Error(`redirects.json entry #${index + 1} has invalid "title"`);
  }

  return {
    from,
    to,
    title: title ? title.trim() : to,
  };
}

/**
 * Load and validate a redirect manifest JSON file.
 */
export function loadRedirects(
  manifestPath = DEFAULT_MANIFEST_PATH,
  { basePath = DEFAULT_BASE_PATH } = {},
) {
  if (!existsSync(manifestPath)) {
    throw new Error(`Missing redirect manifest at ${manifestPath}`);
  }
  const parsed = JSON.parse(readFileSync(manifestPath, 'utf8'));
  if (!Array.isArray(parsed) || parsed.length === 0) {
    throw new Error(`Redirect manifest ${manifestPath} must be a non-empty JSON array`);
  }

  const seen = new Set();
  return parsed.map((raw, index) => {
    const entry = validateRedirectEntry(raw, index, { basePath });
    const key = entry.from.toLowerCase();
    if (seen.has(key)) {
      throw new Error(`Duplicate redirect source in ${manifestPath}: ${entry.from}`);
    }
    seen.add(key);
    return entry;
  });
}

/**
 * Render a minimal, self-contained HTML5 redirect stub that preserves
 * `?query` and `#fragment` (including HELP_BASE_URL `#wfxxxx` anchors).
 */
export function renderRedirectHtml({ to, title, siteOrigin = DEFAULT_SITE_ORIGIN }) {
  const origin = siteOrigin.replace(/\/+$/, '');
  const canonical = `${origin}${to}`;
  const label = title ? String(title).trim() : to;
  const escapedLabel = escapeHtml(label);
  const escapedTo = escapeHtml(to);
  const escapedCanonical = escapeHtml(canonical);
  const jsTarget = escapeJsString(to);

  return [
    '<!DOCTYPE html>',
    REDIRECT_MARKER,
    '<html lang="en">',
    '<head>',
    '  <meta charset="utf-8">',
    '  <meta name="viewport" content="width=device-width, initial-scale=1">',
    '  <meta name="robots" content="noindex">',
    `  <title>Redirecting to ${escapedLabel} | witforge</title>`,
    `  <link rel="canonical" href="${escapedCanonical}">`,
    `  <noscript><meta http-equiv="refresh" content="0; url=${escapedTo}"></noscript>`,
    '  <script>',
    '    (function () {',
    `      var target = ${jsTarget};`,
    '      window.location.replace(target + window.location.search + window.location.hash);',
    '    })();',
    '  </script>',
    '  <style>',
    '    :root { color-scheme: light dark; }',
    '    body { margin: 0; font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; line-height: 1.5; padding: 3rem 1.25rem; color: #0f172a; background: #ffffff; }',
    '    main { max-width: 36rem; margin: 0 auto; }',
    '    h1 { font-size: 1.25rem; margin: 0 0 0.5rem; }',
    '    p { margin: 0; color: #334155; }',
    '    a { color: #1d4ed8; font-weight: 600; text-underline-offset: 3px; }',
    '    @media (prefers-color-scheme: dark) {',
    '      body { color: #e2e8f0; background: #0a0f1a; }',
    '      p { color: #94a3b8; }',
    '      a { color: #60a5fa; }',
    '    }',
    '  </style>',
    '</head>',
    '<body>',
    '  <main>',
    '    <h1>Page moved</h1>',
    `    <p>This page has moved to <a href="${escapedTo}">${escapedLabel}</a>.</p>`,
    '  </main>',
    '</body>',
    '</html>',
    '',
  ].join('\n');
}

function resolveInsideDir(rootDir, relSegments, label) {
  const absRoot = resolve(rootDir);
  const resolved = resolve(absRoot, ...relSegments);
  const rel = toPosix(relative(absRoot, resolved));
  if (!rel || rel === '..' || rel.startsWith('../')) {
    throw new Error(`Path escapes dist directory (${label}): ${relSegments.join('/')}`);
  }
  return { resolved, rel };
}

/**
 * Emit all legacy compatibility redirect HTML stubs into `site/dist/`.
 *
 * Validates every destination page and collision check before writing any file.
 */
export function emitRedirects({
  distDir = DEFAULT_DIST_DIR,
  manifestPath = DEFAULT_MANIFEST_PATH,
  siteOrigin = DEFAULT_SITE_ORIGIN,
  basePath = DEFAULT_BASE_PATH,
} = {}) {
  const absDist = resolve(distDir);
  if (!existsSync(absDist) || !statSync(absDist).isDirectory()) {
    throw new Error(`Built dist directory not found: ${absDist}`);
  }

  const normalizedBase = basePath.replace(/\/+$/, '');
  const entries = loadRedirects(manifestPath, { basePath: normalizedBase });
  const planned = [];

  for (const entry of entries) {
    const destSubpath = entry.to.slice(normalizedBase.length).replace(/^\/+|\/+$/g, '');
    const destSegments = destSubpath ? [...destSubpath.split('/'), 'index.html'] : ['index.html'];
    const { resolved: destFile, rel: destRel } = resolveInsideDir(absDist, destSegments, entry.to);
    if (!existsSync(destFile) || !statSync(destFile).isFile()) {
      throw new Error(
        `Redirect target does not exist in dist: ${entry.from} -> ${entry.to} (missing ${destRel})`,
      );
    }

    const fromSegments = entry.from.replace(/^\/+/, '').split('/');
    const { resolved: outFile, rel: outRel } = resolveInsideDir(absDist, fromSegments, entry.from);
    if (existsSync(outFile)) {
      const existingStat = statSync(outFile);
      if (!existingStat.isFile()) {
        throw new Error(`Redirect source collides with non-file path in dist: ${outRel}`);
      }
      const existing = readFileSync(outFile, 'utf8');
      if (!existing.includes(REDIRECT_MARKER)) {
        throw new Error(
          `Redirect source collides with an existing canonical route in dist: ${outRel}`,
        );
      }
    }

    planned.push({
      entry,
      outFile,
      outRel,
      html: renderRedirectHtml({ to: entry.to, title: entry.title, siteOrigin }),
    });
  }

  for (const item of planned) {
    mkdirSync(dirname(item.outFile), { recursive: true });
    writeFileSync(item.outFile, item.html, 'utf8');
  }

  return {
    count: planned.length,
    written: planned.map((item) => item.outRel),
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const result = emitRedirects();
  console.log(`Emitted ${result.count} legacy compatibility redirects into site/dist/.`);
}
