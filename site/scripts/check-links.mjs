import { existsSync, readdirSync, readFileSync, realpathSync, statSync } from 'node:fs';
import { dirname, isAbsolute, join, posix, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  DEFAULT_BASE_PATH,
  DEFAULT_SITE_ORIGIN,
  loadRedirects,
  REDIRECT_HEADER,
} from './emit-redirects.mjs';
import { findEmDashesInHtml, findEmDashesInText, toPosix } from './markdown-utils.mjs';

const SCRIPTS_DIR = dirname(fileURLToPath(import.meta.url));
const DEFAULT_SITE_ROOT = resolve(SCRIPTS_DIR, '..');
const DEFAULT_DIST_DIR = resolve(DEFAULT_SITE_ROOT, 'dist');
const DEFAULT_REPO_ROOT = resolve(DEFAULT_SITE_ROOT, '..');
const DEFAULT_DOCS_ROOT = resolve(DEFAULT_REPO_ROOT, 'docs');
const DEFAULT_MANIFEST_PATH = resolve(SCRIPTS_DIR, 'redirects.json');

const REPO_GITHUB_PREFIX = 'https://github.com/asamarts/witforge/';
const PNG_MAGIC = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
const EN_DASH_RE = /\u2013|&ndash;|&#0*8211;|&#x0*2013;/gi;

export const BANNED_CLICHES = [
  /\bdelve\b/i,
  /\btapestry\b/i,
  /\bseamlessly\b/i,
  /\bseamless\b/i,
  /\bunleash\b/i,
  /\belevate\b/i,
  /\bsupercharge\b/i,
  /\bempower(?:s|ed|ing)?\s+your\b/i,
  /\bgame[- ]changer\b/i,
  /\brevolutionary\b/i,
  /\bcutting[- ]edge\b/i,
  /\bnext[- ]gen\b/i,
  /\bunlock\s+the\s+power\b/i,
  /\bat\s+the\s+end\s+of\s+the\s+day\b/i,
  /\bit\s+is\s+worth\s+noting\b/i,
  /\bever[- ]evolving\b/i,
  /\bdive\s+into\b/i,
];

const EXTERNAL_SCHEME_RE = /^(?:mailto:|tel:|data:|javascript:|blob:)/i;

/**
 * Decode common HTML character references into plain text.
 */
export function decodeHtmlEntities(text) {
  return String(text)
    .replace(/&#x([0-9a-f]+);/gi, (_, hex) => String.fromCodePoint(Number.parseInt(hex, 16)))
    .replace(/&#(\d+);/g, (_, dec) => String.fromCodePoint(Number.parseInt(dec, 10)))
    .replace(/&quot;/g, '"')
    .replace(/&apos;|&#39;/g, "'")
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&nbsp;/g, ' ')
    .replace(/&amp;/g, '&');
}

function stripPairedTagBlocks(src, tagName) {
  const re = new RegExp(
    `<${tagName}\\b[^>]*>(?:(?!<${tagName}\\b)[\\s\\S])*?<\\/${tagName}\\s*>`,
    'gi',
  );
  let prev = src;
  while (true) {
    const next = prev.replace(re, (block) => block.replace(/[^\n]/g, ' '));
    if (next === prev) {
      return next;
    }
    prev = next;
  }
}

function stripScriptsStylesAndComments(html) {
  let out = String(html).replace(/\r\n/g, '\n');
  out = stripPairedTagBlocks(out, 'script');
  out = stripPairedTagBlocks(out, 'style');
  out = out.replace(/<!--[\s\S]*?-->/g, (comment) => comment.replace(/[^\n]/g, ' '));
  return out;
}

function stripNonProseHtml(html) {
  let out = stripScriptsStylesAndComments(html);
  out = stripPairedTagBlocks(out, 'code');
  out = stripPairedTagBlocks(out, 'pre');
  return out;
}

/**
 * Extract all element `id` attribute values from HTML (ignoring scripts, styles, and comments).
 *
 * @param {string} html
 * @returns {Set<string>}
 */
export function extractElementIds(html) {
  const clean = stripScriptsStylesAndComments(html);
  const ids = new Set();
  const re = /<[a-z][a-z0-9-]*\b[^>]*?\bid=(?:"([^"]+)"|'([^']+)')/gi;
  let match;
  while ((match = re.exec(clean)) !== null) {
    const raw = match[1] ?? match[2] ?? '';
    if (raw) {
      ids.add(decodeHtmlEntities(raw));
    }
  }
  return ids;
}

/**
 * Extract all `href` and `src` attribute values from HTML tags (ignoring scripts, styles, and comments).
 *
 * @param {string} html
 * @returns {Array<{ tag: string, attr: 'href' | 'src', value: string, relAttr: string }>}
 */
export function extractLinks(html) {
  const clean = stripScriptsStylesAndComments(html);
  const links = [];
  const tagRe = /<([a-z][a-z0-9-]*)\b([^>]*)>/gi;
  let tagMatch;
  while ((tagMatch = tagRe.exec(clean)) !== null) {
    const tag = tagMatch[1].toLowerCase();
    const attrs = tagMatch[2];
    const relMatch = /\brel=(?:"([^"]*)"|'([^']*)')/i.exec(attrs);
    const relAttr = (relMatch?.[1] ?? relMatch?.[2] ?? '').toLowerCase();
    const attrRe = /\b(href|src)=(?:"([^"]*)"|'([^']*)')/gi;
    let attrMatch;
    while ((attrMatch = attrRe.exec(attrs)) !== null) {
      const attr = /** @type {'href' | 'src'} */ (attrMatch[1].toLowerCase());
      const value = decodeHtmlEntities(attrMatch[2] ?? attrMatch[3] ?? '').trim();
      if (value) {
        links.push({ tag, attr, value, relAttr });
      }
    }
  }
  return links;
}

/**
 * Check rendered HTML prose for em dashes, stray en dashes, leaked Markdown table backticks,
 * and banned AI-writing clichés (while ignoring `<pre>`, `<code>`, `<script>`, `<style>`).
 *
 * @param {string} html
 * @returns {string[]}
 */
export function findProseIssuesInHtml(html) {
  const issues = [];
  const src = String(html).replace(/\r\n/g, '\n');

  for (const hit of findEmDashesInHtml(src)) {
    issues.push(`em dash (${hit.match}) at line ${hit.line}: ${hit.excerpt}`);
  }

  const proseHtml = stripNonProseHtml(src);

  // Detect leaked raw backticks inside table cells (catches GFM unescaped pipe truncation bugs).
  const tdRe = /<td\b[^>]*>([\s\S]*?)<\/td>/gi;
  let tdMatch;
  while ((tdMatch = tdRe.exec(proseHtml)) !== null) {
    const cellText = tdMatch[1].replace(/<[^>]+>/g, '');
    if (cellText.includes('`')) {
      const line = proseHtml.slice(0, tdMatch.index).split('\n').length;
      issues.push(
        `leaked backtick in table cell at line ${line}: ${cellText.trim().slice(0, 100)}`,
      );
    }
  }

  // Extract visible text plus meta description/title values for en-dash & cliché checks.
  const textChunks = [];
  const metaRe =
    /<meta\b[^>]*\b(?:name|property)=["'](?:description|og:description|og:title|twitter:description|twitter:title)["'][^>]*>/gi;
  let metaMatch;
  while ((metaMatch = metaRe.exec(src)) !== null) {
    const contentMatch = /\bcontent=(["'])([\s\S]*?)\1/i.exec(metaMatch[0]);
    if (contentMatch) {
      const line = src.slice(0, metaMatch.index).split('\n').length;
      textChunks.push({ line, text: decodeHtmlEntities(contentMatch[2]), context: 'meta tag' });
    }
  }

  const visibleLines = proseHtml.replace(/<[^>]+>/g, ' ').split('\n');
  for (let i = 0; i < visibleLines.length; i += 1) {
    const lineText = visibleLines[i];
    if (lineText.trim()) {
      textChunks.push({ line: i + 1, text: lineText, context: 'prose' });
    }
  }

  for (const chunk of textChunks) {
    EN_DASH_RE.lastIndex = 0;
    let enMatch;
    while ((enMatch = EN_DASH_RE.exec(chunk.text)) !== null) {
      issues.push(
        `en dash (${enMatch[0]}) in ${chunk.context} at line ${chunk.line}: ${chunk.text.trim().slice(0, 120)}`,
      );
    }
    for (const pattern of BANNED_CLICHES) {
      const m = pattern.exec(chunk.text);
      if (m) {
        issues.push(
          `banned prose cliché (${JSON.stringify(m[0])}) in ${chunk.context} at line ${chunk.line}: ${chunk.text.trim().slice(0, 120)}`,
        );
      }
    }
  }

  return issues;
}

/**
 * Check plain Markdown/text files (such as `llms.txt` and `llms-full.txt`) for em dashes
 * and banned AI-writing clichés (skipping fenced code blocks for cliché scanning).
 *
 * @param {string} text
 * @returns {string[]}
 */
export function findProseIssuesInPlainText(text) {
  const issues = [];
  const normalized = String(text).replace(/\r\n/g, '\n');

  for (const hit of findEmDashesInText(normalized)) {
    issues.push(`em dash (${hit.match}) at line ${hit.line}: ${hit.excerpt}`);
  }

  const lines = normalized.split('\n');
  let inFence = false;
  let fenceChar = '';
  let fenceLen = 0;

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    const trimmed = line.trimStart();
    if (!inFence) {
      const m = /^(`{3,}|~{3,})/.exec(trimmed);
      if (m) {
        inFence = true;
        fenceChar = m[1][0];
        fenceLen = m[1].length;
        continue;
      }
      const noInlineCode = line.replace(/`[^`]*`/g, ' ');
      for (const pattern of BANNED_CLICHES) {
        const hit = pattern.exec(noInlineCode);
        if (hit) {
          issues.push(
            `banned prose cliché (${JSON.stringify(hit[0])}) at line ${i + 1}: ${line.trim().slice(0, 120)}`,
          );
        }
      }
    } else {
      const closeRe = new RegExp(`^${fenceChar}{${fenceLen},}\\s*$`);
      if (closeRe.test(trimmed)) {
        inFence = false;
      }
    }
  }

  return issues;
}

function getAttr(attrs, name) {
  const re = new RegExp(`\\b${name}=(?:"([^"]*)"|'([^']*)')`, 'i');
  const m = re.exec(attrs);
  return m ? decodeHtmlEntities(m[1] ?? m[2] ?? '') : null;
}

function getNumericAttr(attrs, name) {
  const raw = getAttr(attrs, name);
  if (raw === null) {
    return null;
  }
  const num = Number(raw);
  return Number.isFinite(num) ? num : null;
}

function checkPointInBounds(x, y, bounds, desc, errors) {
  const tol = 2;
  if (
    x < bounds.minX - tol ||
    x > bounds.maxX + tol ||
    y < bounds.minY - tol ||
    y > bounds.maxY + tol
  ) {
    errors.push(
      `${desc} (${x}, ${y}) lies outside SVG viewBox [${bounds.minX}, ${bounds.minY}, ${bounds.maxX}, ${bounds.maxY}]`,
    );
  }
}

function checkPathBounds(dAttr, bounds, label, errors) {
  const tokens = dAttr.match(/[A-Za-z]|[-+]?(?:\d+\.?\d*|\.\d+)(?:e[-+]?\d+)?/g);
  if (!tokens) {
    return;
  }
  let idx = 0;
  let cmd = '';
  let curX = 0;
  let curY = 0;

  const readNum = () => {
    if (idx >= tokens.length || /^[A-Za-z]$/.test(tokens[idx])) {
      return null;
    }
    const n = Number(tokens[idx]);
    idx += 1;
    return Number.isFinite(n) ? n : null;
  };

  while (idx < tokens.length) {
    if (/^[A-Za-z]$/.test(tokens[idx])) {
      cmd = tokens[idx];
      idx += 1;
      if (cmd === 'Z' || cmd === 'z') {
        continue;
      }
    }
    if (!cmd) {
      break;
    }
    if (cmd === 'M' || cmd === 'L' || cmd === 'T') {
      const x = readNum();
      const y = readNum();
      if (x === null || y === null) break;
      curX = x;
      curY = y;
      checkPointInBounds(curX, curY, bounds, `${label} <path> ${cmd}`, errors);
      if (cmd === 'M') cmd = 'L';
    } else if (cmd === 'm' || cmd === 'l' || cmd === 't') {
      const dx = readNum();
      const dy = readNum();
      if (dx === null || dy === null) break;
      curX += dx;
      curY += dy;
      checkPointInBounds(curX, curY, bounds, `${label} <path> ${cmd}`, errors);
      if (cmd === 'm') cmd = 'l';
    } else if (cmd === 'H') {
      const x = readNum();
      if (x === null) break;
      curX = x;
      checkPointInBounds(curX, curY, bounds, `${label} <path> H`, errors);
    } else if (cmd === 'h') {
      const dx = readNum();
      if (dx === null) break;
      curX += dx;
      checkPointInBounds(curX, curY, bounds, `${label} <path> h`, errors);
    } else if (cmd === 'V') {
      const y = readNum();
      if (y === null) break;
      curY = y;
      checkPointInBounds(curX, curY, bounds, `${label} <path> V`, errors);
    } else if (cmd === 'v') {
      const dy = readNum();
      if (dy === null) break;
      curY += dy;
      checkPointInBounds(curX, curY, bounds, `${label} <path> v`, errors);
    } else if (cmd === 'C') {
      const x1 = readNum();
      const y1 = readNum();
      const x2 = readNum();
      const y2 = readNum();
      const x = readNum();
      const y = readNum();
      if (x === null || y === null || x1 === null || y1 === null || x2 === null || y2 === null) {
        break;
      }
      checkPointInBounds(x1, y1, bounds, `${label} <path> C cp1`, errors);
      checkPointInBounds(x2, y2, bounds, `${label} <path> C cp2`, errors);
      curX = x;
      curY = y;
      checkPointInBounds(curX, curY, bounds, `${label} <path> C`, errors);
    } else if (cmd === 'S' || cmd === 'Q') {
      const x1 = readNum();
      const y1 = readNum();
      const x = readNum();
      const y = readNum();
      if (x === null || y === null || x1 === null || y1 === null) break;
      checkPointInBounds(x1, y1, bounds, `${label} <path> ${cmd} cp`, errors);
      curX = x;
      curY = y;
      checkPointInBounds(curX, curY, bounds, `${label} <path> ${cmd}`, errors);
    } else {
      // Skip less common commands cleanly without infinite looping
      idx += 1;
    }
  }
}

/**
 * Validate inline SVG diagrams in an HTML document.
 *
 * Targets every `<svg>` inside `<figure>`, carrying `role="img"`, or with a `viewBox` width >= 200
 * (so decorative 16-32px UI icons remain exempt while forgotten diagram attributes are caught).
 *
 * @param {string} html
 * @param {string} relFile
 * @returns {{ diagramCount: number, errors: string[] }}
 */
export function validateSvgDiagramsInHtml(html, relFile) {
  const clean = stripScriptsStylesAndComments(html);
  const errors = [];
  let diagramCount = 0;

  // Record character ranges of `<figure>...</figure>` so SVGs inside figures are always checked.
  const figureRanges = [];
  const figRe = /<figure\b[^>]*>[\s\S]*?<\/figure>/gi;
  let figMatch;
  while ((figMatch = figRe.exec(clean)) !== null) {
    figureRanges.push([figMatch.index, figMatch.index + figMatch[0].length]);
  }
  const inFigure = (pos) => figureRanges.some(([start, end]) => pos >= start && pos < end);

  const svgRe = /<svg\b([^>]*)>([\s\S]*?)<\/svg>/gi;
  let svgMatch;
  while ((svgMatch = svgRe.exec(clean)) !== null) {
    const attrs = svgMatch[1];
    const inner = svgMatch[2];
    const role = getAttr(attrs, 'role');
    const ariaHidden = getAttr(attrs, 'aria-hidden');
    const viewBoxRaw = getAttr(attrs, 'viewBox');
    const viewBoxParts = viewBoxRaw ? viewBoxRaw.trim().split(/[\s,]+/).map(Number) : null;
    const isLargeSvg =
      viewBoxParts !== null &&
      viewBoxParts.length === 4 &&
      Number.isFinite(viewBoxParts[2]) &&
      viewBoxParts[2] >= 200;

    const isDiagram = role === 'img' || (inFigure(svgMatch.index) && ariaHidden !== 'true') || isLargeSvg;
    if (!isDiagram) {
      continue;
    }

    diagramCount += 1;
    const label = `${relFile} SVG diagram #${diagramCount}`;

    if (role !== 'img') {
      errors.push(`${label}: missing role="img"`);
    }

    const labelledBy = getAttr(attrs, 'aria-labelledby');
    const innerIds = extractElementIds(inner);
    if (!labelledBy || !labelledBy.trim()) {
      errors.push(`${label}: missing aria-labelledby attribute`);
    } else {
      for (const refId of labelledBy.trim().split(/\s+/)) {
        if (!innerIds.has(refId)) {
          errors.push(`${label}: aria-labelledby references missing id "${refId}"`);
        }
      }
    }

    const titleMatch = /<title\b[^>]*>([\s\S]*?)<\/title>/i.exec(inner);
    if (!titleMatch || !titleMatch[1].trim()) {
      errors.push(`${label}: missing non-empty <title> element`);
    }
    const descMatch = /<desc\b[^>]*>([\s\S]*?)<\/desc>/i.exec(inner);
    if (!descMatch || !descMatch[1].trim()) {
      errors.push(`${label}: missing non-empty <desc> element`);
    }

    if (
      !viewBoxParts ||
      viewBoxParts.length !== 4 ||
      viewBoxParts.some((n) => !Number.isFinite(n)) ||
      viewBoxParts[2] <= 0 ||
      viewBoxParts[3] <= 0
    ) {
      errors.push(`${label}: invalid or missing numeric viewBox (${JSON.stringify(viewBoxRaw)})`);
      continue;
    }

    const [minX, minY, vbW, vbH] = viewBoxParts;
    const bounds = { minX, minY, maxX: minX + vbW, maxY: minY + vbH };
    const bodyWithoutDefs = stripPairedTagBlocks(inner, 'defs');

    for (const m of bodyWithoutDefs.matchAll(/<rect\b([^>]*)\/?>/gi)) {
      const a = m[1];
      const x = getNumericAttr(a, 'x') ?? 0;
      const y = getNumericAttr(a, 'y') ?? 0;
      const w = getNumericAttr(a, 'width');
      const h = getNumericAttr(a, 'height');
      if (w !== null && h !== null) {
        checkPointInBounds(x, y, bounds, `${label} <rect> top-left`, errors);
        checkPointInBounds(x + w, y + h, bounds, `${label} <rect> bottom-right`, errors);
      }
    }

    for (const m of bodyWithoutDefs.matchAll(/<circle\b([^>]*)\/?>/gi)) {
      const a = m[1];
      const cx = getNumericAttr(a, 'cx') ?? 0;
      const cy = getNumericAttr(a, 'cy') ?? 0;
      const r = getNumericAttr(a, 'r') ?? 0;
      checkPointInBounds(cx - r, cy - r, bounds, `${label} <circle> min`, errors);
      checkPointInBounds(cx + r, cy + r, bounds, `${label} <circle> max`, errors);
    }

    for (const m of bodyWithoutDefs.matchAll(/<ellipse\b([^>]*)\/?>/gi)) {
      const a = m[1];
      const cx = getNumericAttr(a, 'cx') ?? 0;
      const cy = getNumericAttr(a, 'cy') ?? 0;
      const rx = getNumericAttr(a, 'rx') ?? 0;
      const ry = getNumericAttr(a, 'ry') ?? 0;
      checkPointInBounds(cx - rx, cy - ry, bounds, `${label} <ellipse> min`, errors);
      checkPointInBounds(cx + rx, cy + ry, bounds, `${label} <ellipse> max`, errors);
    }

    for (const m of bodyWithoutDefs.matchAll(/<line\b([^>]*)\/?>/gi)) {
      const a = m[1];
      const x1 = getNumericAttr(a, 'x1');
      const y1 = getNumericAttr(a, 'y1');
      const x2 = getNumericAttr(a, 'x2');
      const y2 = getNumericAttr(a, 'y2');
      if (x1 !== null && y1 !== null) {
        checkPointInBounds(x1, y1, bounds, `${label} <line> start`, errors);
      }
      if (x2 !== null && y2 !== null) {
        checkPointInBounds(x2, y2, bounds, `${label} <line> end`, errors);
      }
    }

    for (const m of bodyWithoutDefs.matchAll(/<text\b([^>]*)>/gi)) {
      const a = m[1];
      const x = getNumericAttr(a, 'x');
      const y = getNumericAttr(a, 'y');
      if (x !== null && y !== null) {
        checkPointInBounds(x, y, bounds, `${label} <text> anchor`, errors);
      }
    }

    for (const m of bodyWithoutDefs.matchAll(/<path\b([^>]*)\/?>/gi)) {
      const dAttr = getAttr(m[1], 'd');
      if (dAttr) {
        checkPathBounds(dAttr, bounds, label, errors);
      }
    }
  }

  return { diagramCount, errors };
}

function collectHtmlFiles(distDir) {
  const out = [];
  function walk(dir) {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      const rel = toPosix(relative(distDir, full));
      if (rel === 'pagefind' || rel.startsWith('pagefind/')) {
        continue;
      }
      if (entry.isDirectory()) {
        walk(full);
      } else if (entry.isFile() && entry.name.endsWith('.html')) {
        out.push(rel);
      }
    }
  }
  walk(distDir);
  return out.sort();
}

function readWorkspaceVersion(repoRoot) {
  const cargoTomlPath = join(repoRoot, 'Cargo.toml');
  if (!existsSync(cargoTomlPath)) {
    return null;
  }
  const raw = readFileSync(cargoTomlPath, 'utf8');
  const section = /\[workspace\.package\][\s\S]*?(?=\n\[|$)/.exec(raw)?.[0] ?? raw;
  return /^\s*version\s*=\s*"([^"]+)"/m.exec(section)?.[1] ?? null;
}

function parseOutputTourPanels(indexHtml) {
  const panels = [];
  const panelRe = /<div\b[^>]*\bid="tour-panel-([^"]+)"[^>]*>([\s\S]*?)<\/div>\s*(?=<div\b[^>]*\bid="tour-panel-|<\/div>\s*<\/div>\s*<\/section>)/gi;
  let match;
  while ((match = panelRe.exec(indexHtml)) !== null) {
    const id = match[1];
    const body = match[2];
    const sourceMatch = /href="https:\/\/github\.com\/asamarts\/witforge\/blob\/main\/(examples\/[^"]+)"/i.exec(
      body,
    );
    const preMatch = /<pre\b[^>]*>([\s\S]*?)<\/pre>/i.exec(body);
    if (sourceMatch && preMatch) {
      const code = decodeHtmlEntities(preMatch[1].replace(/<[^>]+>/g, ''))
        .replace(/\r\n/g, '\n')
        .trimEnd();
      panels.push({ id, sourceRelPath: sourceMatch[1], code });
    }
  }
  return panels;
}

function verifyGitHubRepoUrl(url, sourceLabel, repoRoot, errors) {
  if (!url.startsWith(REPO_GITHUB_PREFIX)) {
    return false;
  }
  const rest = url.slice(REPO_GITHUB_PREFIX.length);
  const match = /^(blob|tree|edit)\/main\/([^?#]+)/.exec(rest);
  if (!match) {
    return false;
  }
  const kind = match[1];
  let relPath;
  try {
    relPath = decodeURIComponent(match[2]);
  } catch {
    errors.push(`${sourceLabel}: malformed percent-encoding in GitHub URL ${url}`);
    return true;
  }
  const segments = relPath.split('/').filter(Boolean);
  if (segments.length === 0 || segments.some((s) => s === '.' || s === '..')) {
    errors.push(`${sourceLabel}: unsafe GitHub repo path in ${url}`);
    return true;
  }
  const absRepo = resolve(repoRoot);
  const target = resolve(absRepo, ...segments);
  const relCheck = toPosix(relative(absRepo, target));
  if (!relCheck || isAbsolute(relCheck) || relCheck === '..' || relCheck.startsWith('../')) {
    errors.push(`${sourceLabel}: GitHub repo URL escapes repository root: ${url}`);
    return true;
  }
  if (!existsSync(target)) {
    errors.push(`${sourceLabel}: GitHub repo link target does not exist: ${relPath} (${url})`);
    return true;
  }
  const st = statSync(target);
  if ((kind === 'blob' || kind === 'edit') && !st.isFile()) {
    errors.push(`${sourceLabel}: GitHub ${kind} link does not point to a file: ${relPath}`);
  } else if (kind === 'tree' && !st.isDirectory()) {
    errors.push(`${sourceLabel}: GitHub tree link does not point to a directory: ${relPath}`);
  }
  return true;
}

/**
 * Run all eight post-build verification categories against a built site directory.
 *
 * @param {{
 *   distDir?: string,
 *   repoRoot?: string,
 *   docsRoot?: string,
 *   manifestPath?: string,
 *   siteOrigin?: string,
 *   basePath?: string,
 *   checkExamples?: boolean,
 *   checkArtifacts?: boolean,
 *   checkDiagnostics?: boolean,
 *   checkRedirects?: boolean,
 *   minDiagrams?: number,
 * }} [options]
 */
export function checkSite({
  distDir = DEFAULT_DIST_DIR,
  repoRoot = DEFAULT_REPO_ROOT,
  docsRoot = DEFAULT_DOCS_ROOT,
  manifestPath = DEFAULT_MANIFEST_PATH,
  siteOrigin = DEFAULT_SITE_ORIGIN,
  basePath = DEFAULT_BASE_PATH,
  checkExamples = true,
  checkArtifacts = true,
  checkDiagnostics = true,
  checkRedirects = true,
  minDiagrams = 2,
} = {}) {
  const errors = [];
  const absDist = resolve(distDir);
  if (!existsSync(absDist) || !statSync(absDist).isDirectory()) {
    return {
      errors: [`Built dist directory not found: ${absDist}`],
      stats: {},
    };
  }

  const normalizedOrigin = siteOrigin.replace(/\/+$/, '');
  const normalizedBase = basePath === '/' ? '' : basePath.replace(/\/+$/, '');
  const siteBaseUrl = `${normalizedOrigin}${normalizedBase}`;

  // Load redirect manifest dynamically so redirect stubs are exempt from canonical trailing-slash checks.
  let redirectEntries = [];
  const redirectSourceSet = new Set();
  const redirectTargetMap = new Map();
  if (checkRedirects) {
    try {
      redirectEntries = loadRedirects(manifestPath, { basePath: normalizedBase || '/' });
      for (const entry of redirectEntries) {
        const rel = entry.from.replace(/^\/+/, '');
        redirectSourceSet.add(rel);
        redirectTargetMap.set(rel, entry.to);
      }
    } catch (err) {
      errors.push(`Redirect manifest error: ${err.message}`);
    }
  }

  const allHtmlFiles = collectHtmlFiles(absDist);
  const canonicalHtmlFiles = allHtmlFiles.filter((rel) => !redirectSourceSet.has(rel));
  const htmlCache = new Map();
  const idsByFile = new Map();

  for (const rel of allHtmlFiles) {
    const raw = readFileSync(join(absDist, ...rel.split('/')), 'utf8');
    htmlCache.set(rel, raw);
    idsByFile.set(rel, extractElementIds(raw));
  }

  // Stats counters
  let checkedInternalLinks = 0;
  let checkedFragments = 0;
  let checkedRepoLinks = 0;
  let checkedDiagrams = 0;
  let verifiedDiagnosticCodes = 0;
  let verifiedTourPanels = 0;
  let workspaceVersion = null;

  // Resolve an internal route/path to a relative file in distDir, enforcing trailing slashes on pages.
  const resolveInternalTarget = (rawHref, currentFile, isRedirectStub) => {
    let urlPath = rawHref;
    if (urlPath.startsWith(`${siteBaseUrl}/`) || urlPath === siteBaseUrl) {
      urlPath = urlPath.slice(normalizedOrigin.length);
    } else if (/^https?:\/\//i.test(urlPath)) {
      return null; // External URL
    } else if (urlPath.startsWith('//')) {
      return null;
    }

    const hashIdx = urlPath.indexOf('#');
    const fragment = hashIdx === -1 ? '' : urlPath.slice(hashIdx + 1);
    const beforeHash = hashIdx === -1 ? urlPath : urlPath.slice(0, hashIdx);
    const queryIdx = beforeHash.indexOf('?');
    const pathOnly = queryIdx === -1 ? beforeHash : beforeHash.slice(0, queryIdx);

    if (!pathOnly) {
      return { targetFile: currentFile, fragment, isPageRoute: true };
    }

    let distRelPath;
    if (pathOnly.startsWith('/')) {
      if (normalizedBase) {
        if (pathOnly === normalizedBase) {
          errors.push(`${currentFile}: internal link missing trailing slash: "${rawHref}"`);
          return { targetFile: 'index.html', fragment, isPageRoute: true };
        }
        if (!pathOnly.startsWith(`${normalizedBase}/`)) {
          errors.push(
            `${currentFile}: internal root-relative link missing base "${normalizedBase}/": "${rawHref}"`,
          );
          return null;
        }
        distRelPath = pathOnly.slice(normalizedBase.length + 1);
      } else {
        distRelPath = pathOnly.slice(1);
      }
    } else {
      const currentDir = posix.dirname(currentFile);
      distRelPath = posix.normalize(
        currentDir === '.' ? pathOnly : posix.join(currentDir, pathOnly),
      );
      if (distRelPath === '..' || distRelPath.startsWith('../')) {
        errors.push(`${currentFile}: relative link escapes site root: "${rawHref}"`);
        return null;
      }
    }

    // Empty means site root `/` -> `index.html`.
    if (!distRelPath) {
      return { targetFile: 'index.html', fragment, isPageRoute: true };
    }

    if (distRelPath.endsWith('/')) {
      const candidate = `${distRelPath}index.html`;
      if (!existsSync(join(absDist, ...candidate.split('/')))) {
        errors.push(`${currentFile}: broken internal link "${rawHref}" (missing ${candidate})`);
        return null;
      }
      return { targetFile: candidate, fragment, isPageRoute: true };
    }

    // Check if it points to a file with extension (asset, xml, txt, or redirect stub).
    const baseName = posix.basename(distRelPath);
    const hasExtension = baseName.includes('.');
    const directAbs = join(absDist, ...distRelPath.split('/'));
    if (hasExtension) {
      if (!existsSync(directAbs) || !statSync(directAbs).isFile()) {
        errors.push(`${currentFile}: broken internal asset/file link "${rawHref}" (missing ${distRelPath})`);
        return null;
      }
      if (!isRedirectStub && redirectSourceSet.has(distRelPath)) {
        errors.push(
          `${currentFile}: canonical page links to legacy redirect stub "${rawHref}" instead of "${redirectTargetMap.get(distRelPath)}"`,
        );
      }
      return { targetFile: distRelPath, fragment, isPageRoute: distRelPath.endsWith('.html') };
    }

    // No extension and no trailing slash -> check if a canonical directory route exists and fail trailing-slash check.
    const dirIndex = `${distRelPath}/index.html`;
    if (existsSync(join(absDist, ...dirIndex.split('/')))) {
      errors.push(`${currentFile}: canonical page link missing trailing slash: "${rawHref}"`);
      return { targetFile: dirIndex, fragment, isPageRoute: true };
    }

    errors.push(`${currentFile}: broken internal link "${rawHref}" (missing ${distRelPath})`);
    return null;
  };

  // Checks 1, 2 (fragments), 4 (GitHub links), 6 (SVG diagrams), and 7 (prose/em-dash/cliché) across HTML.
  for (const rel of allHtmlFiles) {
    const raw = htmlCache.get(rel);
    const isRedirectStub = redirectSourceSet.has(rel);

    // Check 7: Prose hygiene (em dashes, en dashes, leaked table backticks, AI clichés).
    for (const issue of findProseIssuesInHtml(raw)) {
      errors.push(`${rel}: ${issue}`);
    }

    // Check 6: SVG diagram hygiene on canonical pages.
    if (!isRedirectStub) {
      const svgResult = validateSvgDiagramsInHtml(raw, rel);
      checkedDiagrams += svgResult.diagramCount;
      errors.push(...svgResult.errors);
    }

    // Checks 1, 2, 4: Internal links, trailing slashes, fragments, and GitHub repo links.
    const links = extractLinks(raw);
    for (const link of links) {
      if (EXTERNAL_SCHEME_RE.test(link.value)) {
        continue;
      }

      if (verifyGitHubRepoUrl(link.value, rel, repoRoot, errors)) {
        checkedRepoLinks += 1;
        continue;
      }

      // Skip external non-site URLs.
      if (
        /^https?:\/\//i.test(link.value) &&
        !link.value.startsWith(`${siteBaseUrl}/`) &&
        link.value !== siteBaseUrl
      ) {
        continue;
      }

      // Ignore canonical/og:url self-reference on 404.html if Astro omits or points at /404.html.
      const resolved = resolveInternalTarget(link.value, rel, isRedirectStub);
      if (!resolved) {
        continue;
      }
      checkedInternalLinks += 1;

      if (resolved.fragment) {
        checkedFragments += 1;
        let fragTargetFile = resolved.targetFile;
        if (redirectTargetMap.has(fragTargetFile)) {
          const destRoute = redirectTargetMap.get(fragTargetFile);
          const sub = destRoute.slice(normalizedBase.length).replace(/^\/+|\/+$/g, '');
          fragTargetFile = sub ? `${sub}/index.html` : 'index.html';
        }
        const targetIds = idsByFile.get(fragTargetFile);
        if (!targetIds) {
          errors.push(
            `${rel}: fragment link "${link.value}" targets non-HTML or missing page "${fragTargetFile}"`,
          );
        } else {
          let decodedFrag = resolved.fragment;
          try {
            decodedFrag = decodeURIComponent(resolved.fragment);
          } catch {
            // Fall back to raw fragment comparison
          }
          if (!targetIds.has(decodedFrag) && !targetIds.has(resolved.fragment)) {
            errors.push(
              `${rel}: broken fragment "#${resolved.fragment}" in "${link.value}" (not found in ${fragTargetFile})`,
            );
          }
        }
      }
    }
  }

  if (checkedDiagrams < minDiagrams) {
    errors.push(
      `Expected at least ${minDiagrams} verified inline SVG diagram(s), found ${checkedDiagrams}`,
    );
  }

  // Check 2b: Every ### WFxxxx heading in docs/guide/diagnostics.md has id="wfxxxx" in built diagnostics page.
  if (checkDiagnostics) {
    const diagMdPath = join(docsRoot, 'guide', 'diagnostics.md');
    const diagHtmlRel = 'docs/diagnostics/index.html';
    if (!existsSync(diagMdPath)) {
      errors.push(`Missing diagnostic source doc: ${diagMdPath}`);
    } else if (!idsByFile.has(diagHtmlRel)) {
      errors.push(`Missing built diagnostics page: ${diagHtmlRel}`);
    } else {
      const diagMd = readFileSync(diagMdPath, 'utf8');
      const codes = [...diagMd.matchAll(/^###\s+(WF\d{4})\s*$/gm)].map((m) => m[1]);
      if (codes.length === 0) {
        errors.push(`No ### WFxxxx headings found in ${diagMdPath}`);
      }
      const diagIds = idsByFile.get(diagHtmlRel);
      for (const code of codes) {
        const lower = code.toLowerCase();
        if (!diagIds.has(lower)) {
          errors.push(
            `${diagHtmlRel}: missing lowercase anchor id="${lower}" for HELP_BASE_URL code ${code}`,
          );
        } else {
          verifiedDiagnosticCodes += 1;
        }
      }
    }
  }

  // Check 3: Legacy redirect stubs integrity & sitemap exclusion.
  if (checkRedirects) {
    for (const entry of redirectEntries) {
      const stubRel = entry.from.replace(/^\/+/, '');
      const stubHtml = htmlCache.get(stubRel);
      if (!stubHtml) {
        errors.push(`Missing built legacy redirect stub: ${stubRel} -> ${entry.to}`);
        continue;
      }
      if (!stubHtml.replace(/\r\n/g, '\n').startsWith(REDIRECT_HEADER)) {
        errors.push(`${stubRel}: missing witforge legacy redirect header`);
      }
      if (!stubHtml.includes('<meta name="robots" content="noindex">')) {
        errors.push(`${stubRel}: missing <meta name="robots" content="noindex">`);
      }
      const expectedCanonical = `${normalizedOrigin}${entry.to}`;
      if (!stubHtml.includes(`<link rel="canonical" href="${expectedCanonical}">`)) {
        errors.push(`${stubRel}: missing exact absolute canonical link "${expectedCanonical}"`);
      }
      if (
        !stubHtml.includes(
          'window.location.replace(target + window.location.search + window.location.hash);',
        )
      ) {
        errors.push(`${stubRel}: missing query/fragment-preserving window.location.replace`);
      }
      if (
        !stubHtml.includes(
          `<noscript><meta http-equiv="refresh" content="0; url=${entry.to}"></noscript>`,
        )
      ) {
        errors.push(`${stubRel}: missing <noscript> meta refresh to "${entry.to}"`);
      }
      if (!stubHtml.includes(`<a href="${entry.to}">`)) {
        errors.push(`${stubRel}: missing fallback anchor link to "${entry.to}"`);
      }
      const destSub = entry.to.slice(normalizedBase.length).replace(/^\/+|\/+$/g, '');
      const destRel = destSub ? `${destSub}/index.html` : 'index.html';
      if (!htmlCache.has(destRel)) {
        errors.push(`${stubRel}: redirect target "${entry.to}" does not exist (${destRel})`);
      }
    }

    const sitemapChunkPath = join(absDist, 'sitemap-0.xml');
    if (existsSync(sitemapChunkPath)) {
      const sitemapXml = readFileSync(sitemapChunkPath, 'utf8');
      for (const entry of redirectEntries) {
        const legacyLoc = `${normalizedOrigin}${normalizedBase}${entry.from}`;
        if (sitemapXml.includes(legacyLoc)) {
          errors.push(`sitemap-0.xml must not include legacy redirect stub: ${legacyLoc}`);
        }
      }
    }
  }

  // Check 5 & 7b: Required site artifacts + llms*.txt prose & GitHub repo link checks.
  if (checkArtifacts) {
    const requiredFiles = [
      'robots.txt',
      '404.html',
      'favicon.svg',
      'og-card.svg',
      'og-card.png',
      'llms.txt',
      'llms-full.txt',
    ];
    for (const reqRel of requiredFiles) {
      const full = join(absDist, ...reqRel.split('/'));
      if (!existsSync(full) || !statSync(full).isFile() || statSync(full).size === 0) {
        errors.push(`Missing or empty required site artifact: ${reqRel}`);
      }
    }

    const ogPngPath = join(absDist, 'og-card.png');
    if (existsSync(ogPngPath) && statSync(ogPngPath).isFile()) {
      const header = readFileSync(ogPngPath).subarray(0, 8);
      if (!header.equals(PNG_MAGIC)) {
        errors.push('og-card.png does not have a valid PNG header');
      }
    }

    const robotsPath = join(absDist, 'robots.txt');
    if (existsSync(robotsPath)) {
      const robotsTxt = readFileSync(robotsPath, 'utf8');
      if (robotsTxt.includes('sitemap-index.xml')) {
        const idxPath = join(absDist, 'sitemap-index.xml');
        if (!existsSync(idxPath) || statSync(idxPath).size === 0) {
          errors.push('robots.txt references sitemap-index.xml, but dist/sitemap-index.xml is missing');
        } else {
          const idxXml = readFileSync(idxPath, 'utf8');
          for (const m of idxXml.matchAll(/<loc>([^<]+)<\/loc>/gi)) {
            const loc = m[1].trim();
            if (loc.startsWith(`${siteBaseUrl}/`)) {
              const subRel = loc.slice(`${siteBaseUrl}/`.length);
              const subFull = join(absDist, ...subRel.split('/'));
              if (!existsSync(subFull) || statSync(subFull).size === 0) {
                errors.push(`sitemap-index.xml references missing sitemap file: ${subRel}`);
              }
            }
          }
        }
      }
    }

    // Pagefind search index verification
    const pagefindDir = join(absDist, 'pagefind');
    if (!existsSync(pagefindDir) || !statSync(pagefindDir).isDirectory()) {
      errors.push('Missing required Pagefind search index directory: pagefind/');
    } else {
      for (const pfReq of ['pagefind.js', 'pagefind-entry.json']) {
        const pfFile = join(pagefindDir, pfReq);
        if (!existsSync(pfFile) || statSync(pfFile).size === 0) {
          errors.push(`Missing or empty Pagefind artifact: pagefind/${pfReq}`);
        }
      }
      const fragDir = join(pagefindDir, 'fragment');
      if (!existsSync(fragDir) || readdirSync(fragDir).length === 0) {
        errors.push('Pagefind index contains no page fragments under pagefind/fragment/');
      }
    }

    // Check llms.txt & llms-full.txt prose hygiene, site URLs, and GitHub links.
    for (const llmsRel of ['llms.txt', 'llms-full.txt']) {
      const llmsPath = join(absDist, llmsRel);
      if (existsSync(llmsPath)) {
        const text = readFileSync(llmsPath, 'utf8');
        for (const issue of findProseIssuesInPlainText(text)) {
          errors.push(`${llmsRel}: ${issue}`);
        }
        for (const m of text.matchAll(/https:\/\/github\.com\/asamarts\/witforge\/(?:blob|tree|edit)\/main\/[^\s)>"]+/g)) {
          const url = m[0].replace(/[.,;:]+$/, '');
          if (verifyGitHubRepoUrl(url, llmsRel, repoRoot, errors)) {
            checkedRepoLinks += 1;
          }
        }
        const escapedSiteBase = siteBaseUrl.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
        const siteUrlRe = new RegExp(`${escapedSiteBase}(?:/[^\\s)>"]*)?`, 'g');
        for (const m of text.matchAll(siteUrlRe)) {
          const url = m[0].replace(/[.,;:]+$/, '');
          if (resolveInternalTarget(url, llmsRel, false)) {
            checkedInternalLinks += 1;
          }
        }
      }
    }
  }

  // Check 8: Live Cargo.toml workspace version & landing-page Output Tour example sync.
  if (checkExamples) {
    workspaceVersion = readWorkspaceVersion(repoRoot);
    if (!workspaceVersion) {
      errors.push(`Unable to read [workspace.package].version from ${join(repoRoot, 'Cargo.toml')}`);
    } else {
      const indexHtml = htmlCache.get('index.html');
      if (!indexHtml || !indexHtml.includes(`v${workspaceVersion}`)) {
        errors.push(`index.html does not reflect live workspace version v${workspaceVersion}`);
      }
      for (const llmsRel of ['llms.txt', 'llms-full.txt']) {
        const llmsPath = join(absDist, llmsRel);
        if (existsSync(llmsPath)) {
          const llmsText = readFileSync(llmsPath, 'utf8');
          if (!llmsText.includes(workspaceVersion)) {
            errors.push(`${llmsRel} does not reflect live workspace version ${workspaceVersion}`);
          }
        }
      }
    }

    const indexHtml = htmlCache.get('index.html');
    if (indexHtml) {
      const totalPanelIds = (indexHtml.match(/\bid="tour-panel-[^"]+"/g) || []).length;
      const panels = parseOutputTourPanels(indexHtml);
      if (panels.length === 0) {
        errors.push('index.html contains no verifiable Output Tour code panels (tour-panel-*)');
      } else if (panels.length !== totalPanelIds) {
        errors.push(
          `index.html declares ${totalPanelIds} tour-panel-* containers but only ${panels.length} had a verifiable source link and code block`,
        );
      }
      let sawSingle = false;
      let sawEvolved = false;
      for (const panel of panels) {
        if (panel.sourceRelPath.startsWith('examples/single/')) sawSingle = true;
        if (panel.sourceRelPath.startsWith('examples/evolved/')) sawEvolved = true;
        const exampleFile = resolve(repoRoot, ...panel.sourceRelPath.split('/'));
        if (!existsSync(exampleFile)) {
          errors.push(
            `Output Tour panel "${panel.id}" references missing example file: ${panel.sourceRelPath}`,
          );
          continue;
        }
        const sourceRaw = readFileSync(exampleFile, 'utf8').replace(/\r\n/g, '\n');
        if (!sourceRaw.includes(panel.code)) {
          errors.push(
            `Output Tour panel "${panel.id}" excerpt drifted from committed ${panel.sourceRelPath}`,
          );
        } else {
          verifiedTourPanels += 1;
        }
      }
      if (!sawSingle || !sawEvolved) {
        errors.push(
          'Output Tour panels must cover both examples/single/ and examples/evolved/ contracts',
        );
      }
    }
  }

  return {
    errors,
    stats: {
      canonicalHtmlPages: canonicalHtmlFiles.length,
      redirectStubs: redirectEntries.length,
      checkedInternalLinks,
      checkedFragments,
      verifiedDiagnosticCodes,
      checkedRepoLinks,
      checkedDiagrams,
      verifiedTourPanels,
      workspaceVersion,
    },
  };
}

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
  const { errors, stats } = checkSite();
  if (errors.length > 0) {
    console.error(`Site verification failed with ${errors.length} error(s):`);
    for (const err of errors) {
      console.error(`  - ${err}`);
    }
    process.exit(1);
  }
  console.log(
    [
      `Site verification passed (v${stats.workspaceVersion}):`,
      `  canonical HTML pages: ${stats.canonicalHtmlPages}`,
      `  legacy redirect stubs: ${stats.redirectStubs}`,
      `  internal links verified: ${stats.checkedInternalLinks} (${stats.checkedFragments} fragments)`,
      `  WFxxxx diagnostic anchors: ${stats.verifiedDiagnosticCodes}`,
      `  GitHub repo links verified: ${stats.checkedRepoLinks}`,
      `  inline SVG diagrams verified: ${stats.checkedDiagrams}`,
      `  Output Tour example excerpts verified: ${stats.verifiedTourPanels}`,
    ].join('\n'),
  );
}
