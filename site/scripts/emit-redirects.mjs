// @ts-check
import { lstatSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { toPosix } from './markdown-utils.mjs';

const SCRIPTS_DIR = dirname(fileURLToPath(import.meta.url));
const DEFAULT_SITE_ROOT = resolve(SCRIPTS_DIR, '..');
const DEFAULT_DIST_DIR = resolve(DEFAULT_SITE_ROOT, 'dist');
const DEFAULT_MANIFEST_PATH = resolve(SCRIPTS_DIR, 'redirects.json');

export const DEFAULT_SITE_ORIGIN = 'https://asamarts.github.io';
export const DEFAULT_BASE_PATH = '/witforge';
export const REDIRECT_MARKER = '<!-- witforge-legacy-redirect -->';
export const REDIRECT_HEADER = `<!DOCTYPE html>\n${REDIRECT_MARKER}\n`;

const RESERVED_SOURCES = new Set(['/index.html', '/404.html']);
const SAFE_SEGMENT_RE = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;

/**
 * @typedef {{ from: string, to: string, title: string }} RedirectEntry
 * @typedef {{ basePath?: string }} RedirectManifestOptions
 * @typedef {{ to: string, title?: string | null, siteOrigin?: string }} RenderRedirectOptions
 * @typedef {{ distDir?: string, manifestPath?: string, siteOrigin?: string, basePath?: string }} EmitRedirectsOptions
 * @typedef {{ count: number, written: string[] }} EmitRedirectsResult
 */

/**
 * Escape HTML special characters for safe inclusion in text nodes and attribute values.
 *
 * @param {unknown} value
 * @returns {string}
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
 *
 * @param {unknown} value
 * @returns {string}
 */
export function escapeJsString(value) {
  return JSON.stringify(String(value))
    .replace(/</g, '\\u003c')
    .replace(/>/g, '\\u003e')
    .replace(/&/g, '\\u0026')
    .replace(/\u2028/g, '\\u2028')
    .replace(/\u2029/g, '\\u2029');
}

/**
 * @param {unknown} basePath
 * @returns {string}
 */
function normalizeBasePath(basePath) {
  if (typeof basePath !== 'string') {
    throw new Error(`Invalid basePath (${JSON.stringify(basePath)}); expected string`);
  }
  if (basePath === '' || basePath === '/') {
    return '';
  }
  if (!basePath.startsWith('/') || basePath.includes('//') || basePath.includes('\\')) {
    throw new Error(`Invalid basePath (${JSON.stringify(basePath)}); expected "/" or "/<segment>"`);
  }
  const trimmed = basePath.replace(/\/+$/, '');
  const segments = trimmed.slice(1).split('/');
  for (const segment of segments) {
    if (!SAFE_SEGMENT_RE.test(segment)) {
      throw new Error(`Invalid basePath segment (${JSON.stringify(segment)}) in ${basePath}`);
    }
  }
  return trimmed;
}

/**
 * @param {unknown} siteOrigin
 * @returns {string}
 */
function normalizeSiteOrigin(siteOrigin) {
  if (typeof siteOrigin !== 'string' || siteOrigin.trim().length === 0) {
    throw new Error(`Invalid siteOrigin (${JSON.stringify(siteOrigin)}); expected http(s) origin`);
  }
  /** @type {URL} */
  let parsed;
  try {
    parsed = new URL(siteOrigin);
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    throw new Error(`Invalid siteOrigin (${JSON.stringify(siteOrigin)}): ${message}`);
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    throw new Error(`Invalid siteOrigin protocol (${parsed.protocol}); expected http: or https:`);
  }
  if (parsed.pathname !== '/' || parsed.search || parsed.hash) {
    throw new Error(
      `Invalid siteOrigin (${JSON.stringify(siteOrigin)}); must not include a path, query, or hash`,
    );
  }
  return parsed.origin;
}

/**
 * @param {string} route
 * @param {string} kind
 * @returns {string[]}
 */
function validateRouteSegments(route, kind) {
  if (route.includes('\\') || route.includes('?') || route.includes('#') || route.includes('//')) {
    throw new Error(`Unsafe ${kind} path: ${route}`);
  }
  if (/[\u0000-\u001f\u007f\s]/.test(route)) {
    throw new Error(`Unsafe ${kind} path: ${route}`);
  }
  const trimmed = route.replace(/^\/+|\/+$/g, '');
  if (!trimmed) {
    return [];
  }
  const segments = trimmed.split('/');
  for (const segment of segments) {
    if (segment === '.' || segment === '..' || !SAFE_SEGMENT_RE.test(segment)) {
      throw new Error(`Unsafe ${kind} path segment (${JSON.stringify(segment)}): ${route}`);
    }
  }
  return segments;
}

/**
 * Validate a single redirect manifest entry.
 *
 * @param {unknown} entry
 * @param {number} index
 * @param {RedirectManifestOptions} [options]
 * @returns {RedirectEntry}
 */
export function validateRedirectEntry(entry, index, { basePath = DEFAULT_BASE_PATH } = {}) {
  const normalizedBase = normalizeBasePath(basePath);
  if (!entry || typeof entry !== 'object' || Array.isArray(entry)) {
    throw new Error(`redirects.json entry #${index + 1} must be an object`);
  }

  const record = /** @type {Record<string, unknown>} */ (entry);
  const { from, to, title } = record;
  if (typeof from !== 'string' || !from.startsWith('/') || !from.endsWith('.html')) {
    throw new Error(
      `redirects.json entry #${index + 1} has invalid "from" (${JSON.stringify(from)}); expected leading "/" and ".html" suffix`,
    );
  }
  const fromSegments = validateRouteSegments(from, '"from"');
  const fileName = fromSegments[fromSegments.length - 1];
  if (!fileName || fileName.length <= '.html'.length) {
    throw new Error(`redirects.json entry #${index + 1} has empty filename stem: ${from}`);
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
  validateRouteSegments(to, '"to"');

  if (title !== undefined && (typeof title !== 'string' || title.trim().length === 0)) {
    throw new Error(`redirects.json entry #${index + 1} has invalid "title"`);
  }

  return {
    from,
    to,
    title: typeof title === 'string' ? title.trim() : to,
  };
}

/**
 * Load and validate a redirect manifest JSON file.
 *
 * @param {string} [manifestPath]
 * @param {RedirectManifestOptions} [options]
 * @returns {RedirectEntry[]}
 */
export function loadRedirects(
  manifestPath = DEFAULT_MANIFEST_PATH,
  { basePath = DEFAULT_BASE_PATH } = {},
) {
  const manifestStat = lstatOrNull(manifestPath);
  if (!manifestStat || !manifestStat.isFile()) {
    throw new Error(`Missing redirect manifest at ${manifestPath}`);
  }
  /** @type {unknown} */
  let parsed;
  try {
    parsed = JSON.parse(readFileSync(manifestPath, 'utf8'));
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    throw new Error(`Invalid JSON in ${manifestPath}: ${message}`);
  }
  if (!Array.isArray(parsed) || parsed.length === 0) {
    throw new Error(`Redirect manifest ${manifestPath} must be a non-empty JSON array`);
  }

  const normalizedBase = normalizeBasePath(basePath);
  /** @type {Set<string>} */
  const seen = new Set();
  const rawEntries = /** @type {unknown[]} */ (parsed);
  const entries = rawEntries.map((raw, index) => {
    const entry = validateRedirectEntry(raw, index, { basePath: normalizedBase });
    const key = entry.from.toLowerCase();
    if (seen.has(key)) {
      throw new Error(`Duplicate redirect source in ${manifestPath}: ${entry.from}`);
    }
    seen.add(key);
    return entry;
  });

  // Reject intra-manifest path prefix collisions (e.g. "/a.html" and "/a.html/b.html")
  for (const entry of entries) {
    const parts = entry.from.slice(1).split('/');
    for (let i = 1; i < parts.length; i += 1) {
      const ancestor = `/${parts.slice(0, i).join('/')}`.toLowerCase();
      if (seen.has(ancestor)) {
        throw new Error(
          `Conflicting redirect paths in ${manifestPath}: ${entry.from} nests under ${ancestor}`,
        );
      }
    }
  }

  return entries;
}

/**
 * Render a minimal, self-contained HTML5 redirect stub that preserves
 * `?query` and `#fragment` (including HELP_BASE_URL `#wfxxxx` anchors).
 *
 * @param {RenderRedirectOptions} options
 * @returns {string}
 */
export function renderRedirectHtml({ to, title, siteOrigin = DEFAULT_SITE_ORIGIN }) {
  const origin = normalizeSiteOrigin(siteOrigin);
  if (typeof to !== 'string' || !to.startsWith('/')) {
    throw new Error(`Invalid redirect target (${JSON.stringify(to)}); expected leading "/"`);
  }
  const canonical = `${origin}${to}`;
  const trimmedTitle = title == null ? '' : String(title).trim();
  const label = trimmedTitle || to;
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

/**
 * @param {string} targetPath
 * @returns {import('node:fs').Stats | null}
 */
function lstatOrNull(targetPath) {
  try {
    return lstatSync(targetPath);
  } catch (err) {
    const code = err && typeof err === 'object' && 'code' in err ? /** @type {{ code?: unknown }} */ (err).code : undefined;
    if (code === 'ENOENT' || code === 'ENOTDIR') {
      return null;
    }
    throw err;
  }
}

/**
 * @param {string} content
 * @returns {boolean}
 */
function isRedirectStubContent(content) {
  return content.replace(/\r\n/g, '\n').startsWith(REDIRECT_HEADER);
}

/**
 * @param {string} rootDir
 * @param {string[]} relSegments
 * @param {string} label
 * @returns {{ resolved: string, rel: string }}
 */
function resolveInsideDir(rootDir, relSegments, label) {
  const absRoot = resolve(rootDir);
  const resolved = resolve(absRoot, ...relSegments);
  const rel = toPosix(relative(absRoot, resolved));
  if (!rel || isAbsolute(rel) || rel === '..' || rel.startsWith('../')) {
    throw new Error(`Path escapes dist directory (${label}): ${relSegments.join('/')}`);
  }
  return { resolved, rel };
}

/**
 * @param {string} absDist
 * @param {string[]} relSegments
 * @param {string} label
 * @returns {void}
 */
function assertSafeAncestors(absDist, relSegments, label) {
  let cursor = absDist;
  for (let i = 0; i < relSegments.length - 1; i += 1) {
    cursor = resolve(cursor, relSegments[i]);
    const st = lstatOrNull(cursor);
    if (!st) {
      return;
    }
    if (st.isSymbolicLink()) {
      throw new Error(`Refusing symlinked directory in dist (${label}): ${toPosix(relative(absDist, cursor))}`);
    }
    if (!st.isDirectory()) {
      throw new Error(
        `Redirect ancestor collides with non-directory path in dist (${label}): ${toPosix(relative(absDist, cursor))}`,
      );
    }
  }
}

/**
 * Emit all legacy compatibility redirect HTML stubs into `site/dist/`.
 *
 * Validates every destination page, symlink safety, and collision check before writing any file.
 *
 * @param {EmitRedirectsOptions} [options]
 * @returns {EmitRedirectsResult}
 */
export function emitRedirects({
  distDir = DEFAULT_DIST_DIR,
  manifestPath = DEFAULT_MANIFEST_PATH,
  siteOrigin = DEFAULT_SITE_ORIGIN,
  basePath = DEFAULT_BASE_PATH,
} = {}) {
  const absDist = resolve(distDir);
  const distStat = lstatOrNull(absDist);
  if (!distStat) {
    throw new Error(`Built dist directory not found: ${absDist}`);
  }
  if (distStat.isSymbolicLink()) {
    throw new Error(`Refusing symlinked dist directory: ${absDist}`);
  }
  if (!distStat.isDirectory()) {
    throw new Error(`Built dist path is not a directory: ${absDist}`);
  }

  const normalizedOrigin = normalizeSiteOrigin(siteOrigin);
  const normalizedBase = normalizeBasePath(basePath);
  const entries = loadRedirects(manifestPath, { basePath: normalizedBase });
  const plannedSources = new Set(entries.map((entry) => entry.from.slice(1).toLowerCase()));
  /** @type {Array<{ entry: RedirectEntry, outFile: string, outRel: string, html: string }>} */
  const planned = [];

  for (const entry of entries) {
    const destSubpath = entry.to.slice(normalizedBase.length).replace(/^\/+|\/+$/g, '');
    const destSegments = destSubpath ? [...destSubpath.split('/'), 'index.html'] : ['index.html'];
    const { resolved: destFile, rel: destRel } = resolveInsideDir(absDist, destSegments, entry.to);
    if (plannedSources.has(destRel.toLowerCase())) {
      throw new Error(
        `Redirect target points to another redirect source (loop or chain): ${entry.from} -> ${entry.to}`,
      );
    }

    assertSafeAncestors(absDist, destSegments, entry.to);
    const destStat = lstatOrNull(destFile);
    if (!destStat) {
      throw new Error(
        `Redirect target does not exist in dist: ${entry.from} -> ${entry.to} (missing ${destRel})`,
      );
    }
    if (destStat.isSymbolicLink()) {
      throw new Error(`Refusing symlinked redirect target in dist: ${destRel}`);
    }
    if (!destStat.isFile()) {
      throw new Error(`Redirect target is not a file in dist: ${destRel}`);
    }
    const destContent = readFileSync(destFile, 'utf8');
    if (isRedirectStubContent(destContent)) {
      throw new Error(
        `Redirect target is itself a redirect stub (loop or chain): ${entry.from} -> ${entry.to}`,
      );
    }

    const fromSegments = entry.from.replace(/^\/+/, '').split('/');
    const { resolved: outFile, rel: outRel } = resolveInsideDir(absDist, fromSegments, entry.from);
    assertSafeAncestors(absDist, fromSegments, entry.from);
    const outStat = lstatOrNull(outFile);
    if (outStat) {
      if (outStat.isSymbolicLink()) {
        throw new Error(`Refusing symlinked redirect output path in dist: ${outRel}`);
      }
      if (!outStat.isFile()) {
        throw new Error(`Redirect source collides with non-file path in dist: ${outRel}`);
      }
      const existing = readFileSync(outFile, 'utf8');
      if (!isRedirectStubContent(existing)) {
        throw new Error(
          `Redirect source collides with an existing canonical route in dist: ${outRel}`,
        );
      }
    }

    planned.push({
      entry,
      outFile,
      outRel,
      html: renderRedirectHtml({
        to: entry.to,
        title: entry.title,
        siteOrigin: normalizedOrigin,
      }),
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

/**
 * @returns {boolean}
 */
function isDirectEntrypoint() {
  if (!process.argv[1]) {
    return false;
  }
  try {
    return realpathSync(fileURLToPath(import.meta.url)) === realpathSync(resolve(process.argv[1]));
  } catch {
    return false;
  }
}

if (isDirectEntrypoint()) {
  const result = emitRedirects();
  console.log(`Emitted ${result.count} legacy compatibility redirects into site/dist/.`);
}
