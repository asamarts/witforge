// @ts-check
import { existsSync, readdirSync, readFileSync, realpathSync, statSync } from 'node:fs';
import { dirname, isAbsolute, join, posix, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  DEFAULT_BASE_PATH,
  DEFAULT_SITE_ORIGIN,
  escapeJsString,
  loadRedirects,
  REDIRECT_HEADER,
} from './emit-redirects.mjs';
import {
  findEmDashesInHtml,
  findEmDashesInText,
  stripHtmlNoise,
  toPosix,
} from './markdown-utils.mjs';

const SCRIPTS_DIR = dirname(fileURLToPath(import.meta.url));
const DEFAULT_SITE_ROOT = resolve(SCRIPTS_DIR, '..');
const DEFAULT_DIST_DIR = resolve(DEFAULT_SITE_ROOT, 'dist');
const DEFAULT_REPO_ROOT = resolve(DEFAULT_SITE_ROOT, '..');
const DEFAULT_DOCS_ROOT = resolve(DEFAULT_REPO_ROOT, 'docs');
const DEFAULT_MANIFEST_PATH = resolve(SCRIPTS_DIR, 'redirects.json');

const REPO_GITHUB_PREFIX = 'https://github.com/asamarts/witforge/';
const PNG_MAGIC = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
const EN_DASH_RE = /\u2013|&ndash;|&#0*8211;|&#x0*2013;/gi;

const TAG_ATTRS_PATTERN = '(?:[^>"\']|"[^"]*"|\'[^\']*\')*';
const ANY_HTML_TAG_RE = new RegExp(`<\\/?[a-z][a-z0-9-]*\\b${TAG_ATTRS_PATTERN}>|<![^>]*>`, 'gi');
const INLINE_PHRASING_TAG_RE = new RegExp(
  `<\\/?(?:a|abbr|b|bdi|bdo|cite|data|del|dfn|em|i|ins|kbd|mark|q|s|samp|small|span|strong|sub|sup|time|u|var)\\b${TAG_ATTRS_PATTERN}>`,
  'gi',
);
const URI_SCHEME_RE = /^[a-z][a-z0-9+.-]*:/i;

export const BANNED_CLICHES = [
  /\bdelv(?:e|es|ed|ing)\b/i,
  /\btapestry\b/i,
  /\bseamlessly\b/i,
  /\bseamless\b/i,
  /\bunleash(?:es|ed|ing)?\b/i,
  /\belevat(?:e|es|ed|ing)\b/i,
  /\bsupercharg(?:e|es|ed|ing)\b/i,
  /\bempower(?:s|ed|ing)?\s+your\b/i,
  /\bgame[- ]chang(?:er|ers|ing)\b/i,
  /\brevolutionary\b/i,
  /\bcutting[- ]edge\b/i,
  /\bnext[- ]gen\b/i,
  /\bunlock(?:s|ed|ing)?\s+the\s+power\b/i,
  /\bat\s+the\s+end\s+of\s+the\s+day\b/i,
  /\bit\s+is\s+worth\s+noting\b/i,
  /\bever[- ]evolving\b/i,
  /\bdiv(?:e|es|ing)\s+into\b/i,
];

/**
 * Decode common HTML character references safely (guarding against invalid code points).
 *
 * @param {string} text
 * @returns {string}
 */
export function decodeHtmlEntities(text) {
  return String(text)
    .replace(/&#x([0-9a-f]+);/gi, (/** @type {string} */ raw, /** @type {string} */ hex) => {
      const cp = Number.parseInt(hex, 16);
      return Number.isInteger(cp) && cp >= 0 && cp <= 0x10ffff ? String.fromCodePoint(cp) : raw;
    })
    .replace(/&#(\d+);/g, (/** @type {string} */ raw, /** @type {string} */ dec) => {
      const cp = Number.parseInt(dec, 10);
      return Number.isInteger(cp) && cp >= 0 && cp <= 0x10ffff ? String.fromCodePoint(cp) : raw;
    })
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&nbsp;/g, ' ')
    .replace(/&amp;/g, '&');
}

/**
 * Strip paired tag blocks (innermost first for nesting tags such as `<code>` or `<defs>`)
 * using quote-aware opening tag matching so `>` inside quoted attributes never truncates.
 *
 * @param {string} src
 * @param {string} tagName
 * @returns {string}
 */
function stripPairedTagBlocks(src, tagName) {
  const re = new RegExp(
    `<${tagName}\\b${TAG_ATTRS_PATTERN}>(?:(?!<${tagName}\\b)[\\s\\S])*?<\\/${tagName}\\s*>`,
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

/**
 * Parse HTML tag attributes from an attribute slice into a lowercase-keyed Map.
 * Supports double-quoted, single-quoted, and unquoted HTML5 values and never confuses
 * hyphenated prefixes (`stroke-width`, `data-id`, `data-href`) with base attribute names.
 *
 * @param {string} attrSlice
 * @returns {Map<string, string>}
 */
function parseTagAttrs(attrSlice) {
  /** @type {Map<string, string>} */
  const attrs = new Map();
  const re = /([^\s"'=<>`/]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`/]+)))?/g;
  let match;
  while ((match = re.exec(attrSlice)) !== null) {
    const name = match[1].toLowerCase();
    const rawVal = match[2] ?? match[3] ?? match[4] ?? '';
    if (!attrs.has(name)) {
      attrs.set(name, decodeHtmlEntities(rawVal));
    }
  }
  return attrs;
}

/**
 * Strip inline Markdown code spans on a single line using CommonMark backtick-run matching.
 *
 * @param {string} line
 * @returns {string}
 */
function stripInlineCodeSpans(line) {
  let out = '';
  let i = 0;
  const n = line.length;
  while (i < n) {
    if (line[i] === '`') {
      let ticks = 0;
      while (i + ticks < n && line[i + ticks] === '`') {
        ticks += 1;
      }
      const closer = '`'.repeat(ticks);
      let pos = i + ticks;
      let found = -1;
      while (pos < n) {
        const idx = line.indexOf(closer, pos);
        if (idx === -1) break;
        const beforeOk = idx === 0 || line[idx - 1] !== '`';
        const afterOk = idx + ticks >= n || line[idx + ticks] !== '`';
        if (beforeOk && afterOk) {
          found = idx;
          break;
        }
        pos = idx + 1;
      }
      if (found !== -1) {
        out += ' ';
        i = found + ticks;
        continue;
      }
      out += line.slice(i, i + ticks);
      i += ticks;
      continue;
    }
    out += line[i];
    i += 1;
  }
  return out;
}

/**
 * Extract all element `id` attribute values from HTML (ignoring scripts, styles, and comments).
 *
 * @param {string} html
 * @returns {Set<string>}
 */
export function extractElementIds(html) {
  const clean = stripHtmlNoise(html);
  /** @type {Set<string>} */
  const ids = new Set();
  const tagRe = new RegExp(`<([a-z][a-z0-9-]*)\\b(${TAG_ATTRS_PATTERN})>`, 'gi');
  let match;
  while ((match = tagRe.exec(clean)) !== null) {
    const attrs = parseTagAttrs(match[2]);
    const id = attrs.get('id');
    if (id) {
      ids.add(id);
    }
  }
  return ids;
}

/**
 * Extract all `href`, `src`, `poster`, `srcset`, and relevant `<meta>` URLs from HTML tags
 * (ignoring scripts, styles, and comments; quote-aware; exact attribute matching).
 *
 * @param {string} html
 * @returns {Array<{ tag: string, attr: string, value: string }>}
 */
export function extractLinks(html) {
  const clean = stripHtmlNoise(html);
  /** @type {Array<{ tag: string, attr: string, value: string }>} */
  const links = [];
  const tagRe = new RegExp(`<([a-z][a-z0-9-]*)\\b(${TAG_ATTRS_PATTERN})>`, 'gi');
  let tagMatch;
  while ((tagMatch = tagRe.exec(clean)) !== null) {
    const tag = tagMatch[1].toLowerCase();
    const attrs = parseTagAttrs(tagMatch[2]);

    for (const attrName of ['href', 'src', 'poster']) {
      const val = attrs.get(attrName)?.trim();
      if (val) {
        links.push({ tag, attr: attrName, value: val });
      }
    }

    const srcset = attrs.get('srcset')?.trim();
    if (srcset) {
      for (const candidate of srcset.split(',')) {
        const url = candidate.trim().split(/\s+/)[0];
        if (url) {
          links.push({ tag, attr: 'srcset', value: url });
        }
      }
    }

    if (tag === 'meta') {
      const content = attrs.get('content')?.trim() ?? '';
      const key = (attrs.get('property') || attrs.get('name') || '').toLowerCase();
      if ((key === 'og:url' || key === 'og:image' || key === 'twitter:image') && content) {
        links.push({ tag, attr: `meta[${key}]`, value: content });
      }
      const httpEquiv = (attrs.get('http-equiv') || '').toLowerCase();
      if (httpEquiv === 'refresh' && content) {
        const urlMatch = /\burl\s*=\s*(?:"([^"]+)"|'([^']+)'|([^\s;]+))/i.exec(content);
        const refreshUrl = (urlMatch?.[1] ?? urlMatch?.[2] ?? urlMatch?.[3] ?? '').trim();
        if (refreshUrl) {
          links.push({ tag, attr: 'meta[refresh]', value: refreshUrl });
        }
      }
    }
  }
  return links;
}

/**
 * Check rendered HTML prose for em dashes, stray en dashes, leaked Markdown table backticks,
 * and banned AI-writing clichés (ignoring `<pre>`, `<code>`, `<script>`, `<style>`, and comments).
 *
 * @param {string} html
 * @returns {string[]}
 */
export function findProseIssuesInHtml(html) {
  /** @type {string[]} */
  const issues = [];
  const cleanNoise = stripHtmlNoise(html);

  for (const hit of findEmDashesInHtml(cleanNoise)) {
    issues.push(`em dash (${hit.match}) at line ${hit.line}: ${hit.excerpt}`);
  }

  let proseHtml = stripPairedTagBlocks(cleanNoise, 'code');
  proseHtml = stripPairedTagBlocks(proseHtml, 'pre');

  // Detect leaked raw backticks inside table cells (`<td>` and `<th>`, outside `<code>`).
  const cellRe = new RegExp(`<(t[dh])\\b${TAG_ATTRS_PATTERN}>([\\s\\S]*?)<\\/\\1\\s*>`, 'gi');
  let cellMatch;
  while ((cellMatch = cellRe.exec(proseHtml)) !== null) {
    const cellText = decodeHtmlEntities(cellMatch[2].replace(ANY_HTML_TAG_RE, ''));
    if (cellText.includes('`')) {
      const line = proseHtml.slice(0, cellMatch.index).split('\n').length;
      issues.push(
        `leaked backtick in table cell at line ${line}: ${cellText.trim().slice(0, 100)}`,
      );
    }
  }

  // Extract meta description/title values on comment-stripped HTML.
  const metaRe = new RegExp(`<meta\\b(${TAG_ATTRS_PATTERN})>`, 'gi');
  let metaMatch;
  while ((metaMatch = metaRe.exec(cleanNoise)) !== null) {
    const attrs = parseTagAttrs(metaMatch[1]);
    const key = (attrs.get('name') || attrs.get('property') || '').toLowerCase();
    if (
      key !== 'description' &&
      key !== 'og:description' &&
      key !== 'og:title' &&
      key !== 'twitter:description' &&
      key !== 'twitter:title'
    ) {
      continue;
    }
    const val = attrs.get('content');
    if (!val) continue;
    const line = cleanNoise.slice(0, metaMatch.index).split('\n').length;
    EN_DASH_RE.lastIndex = 0;
    let enMatch;
    while ((enMatch = EN_DASH_RE.exec(val)) !== null) {
      issues.push(`en dash (${enMatch[0]}) in meta tag at line ${line}: ${val.trim().slice(0, 120)}`);
    }
    for (const pattern of BANNED_CLICHES) {
      const m = pattern.exec(val);
      if (m) {
        issues.push(
          `banned prose cliché (${JSON.stringify(m[0])}) in meta tag at line ${line}: ${val.trim().slice(0, 120)}`,
        );
      }
    }
  }

  // Strip inline phrasing tags without inserting spaces so `<em>cutting</em>-edge` stays intact,
  // then replace block/structural tags with newlines and decode entities.
  const decodedVisible = decodeHtmlEntities(
    proseHtml.replace(INLINE_PHRASING_TAG_RE, '').replace(ANY_HTML_TAG_RE, '\n'),
  );

  // Check en dashes per line for accurate line reporting.
  const visibleLines = decodedVisible.split('\n');
  for (let i = 0; i < visibleLines.length; i += 1) {
    const lineText = visibleLines[i];
    if (!lineText.trim()) continue;
    EN_DASH_RE.lastIndex = 0;
    let enMatch;
    while ((enMatch = EN_DASH_RE.exec(lineText)) !== null) {
      issues.push(
        `en dash (${enMatch[0]}) in prose at line ${i + 1}: ${lineText.trim().slice(0, 120)}`,
      );
    }
  }

  // Scan collapsed prose across line breaks so multi-word clichés wrapping lines are still caught.
  const collapsedProse = decodedVisible.replace(/\s+/g, ' ').trim();
  for (const pattern of BANNED_CLICHES) {
    const globalRe = new RegExp(pattern.source, pattern.flags.includes('g') ? pattern.flags : `${pattern.flags}g`);
    let m;
    while ((m = globalRe.exec(collapsedProse)) !== null) {
      const start = Math.max(0, m.index - 40);
      const excerpt = collapsedProse.slice(start, Math.min(collapsedProse.length, m.index + 80)).trim();
      issues.push(`banned prose cliché (${JSON.stringify(m[0])}) in prose: ${excerpt}`);
    }
  }

  return issues;
}

/**
 * Check plain Markdown/text files (`llms.txt`, `llms-full.txt`) for em dashes, en dashes,
 * and banned AI-writing clichés (skipping fenced code blocks and inline code spans).
 *
 * @param {string} text
 * @returns {string[]}
 */
export function findProseIssuesInPlainText(text) {
  /** @type {string[]} */
  const issues = [];
  const normalized = String(text).replace(/\r\n/g, '\n');
  const lines = normalized.split('\n');
  let inFence = false;
  let fenceChar = '';
  let fenceLen = 0;
  /** @type {string[]} */
  const proseLines = [];

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    const trimmed = line.trimStart();
    if (!inFence) {
      // CommonMark fenced code blocks: backtick fences cannot contain backticks in their info string.
      const backtickFence = /^(`{3,})[^`]*$/.exec(trimmed);
      const tildeFence = /^(~{3,})/.exec(trimmed);
      const fenceMatch = backtickFence || tildeFence;
      if (fenceMatch) {
        inFence = true;
        fenceChar = fenceMatch[1][0];
        fenceLen = fenceMatch[1].length;
        proseLines.push('');
        continue;
      }

      const noInlineCode = stripInlineCodeSpans(line);
      for (const hit of findEmDashesInText(noInlineCode)) {
        issues.push(`em dash (${hit.match}) at line ${i + 1}: ${line.trim().slice(0, 120)}`);
      }
      EN_DASH_RE.lastIndex = 0;
      let enMatch;
      while ((enMatch = EN_DASH_RE.exec(noInlineCode)) !== null) {
        issues.push(`en dash (${enMatch[0]}) at line ${i + 1}: ${line.trim().slice(0, 120)}`);
      }
      proseLines.push(noInlineCode);
    } else {
      const closeRe = new RegExp(`^${fenceChar}{${fenceLen},}\\s*$`);
      if (closeRe.test(trimmed)) {
        inFence = false;
      }
      proseLines.push('');
    }
  }

  const collapsedProse = proseLines.join('\n').replace(/\s+/g, ' ').trim();
  for (const pattern of BANNED_CLICHES) {
    const globalRe = new RegExp(pattern.source, pattern.flags.includes('g') ? pattern.flags : `${pattern.flags}g`);
    let m;
    while ((m = globalRe.exec(collapsedProse)) !== null) {
      const start = Math.max(0, m.index - 40);
      const excerpt = collapsedProse.slice(start, Math.min(collapsedProse.length, m.index + 80)).trim();
      issues.push(`banned prose cliché (${JSON.stringify(m[0])}) in plain text: ${excerpt}`);
    }
  }

  return issues;
}

/**
 * Parse simple `translate(dx[, dy])` offsets from an SVG `transform` attribute.
 *
 * @param {string | undefined} transformAttr
 * @returns {{ tx: number, ty: number }}
 */
function parseTranslate(transformAttr) {
  if (!transformAttr) {
    return { tx: 0, ty: 0 };
  }
  let tx = 0;
  let ty = 0;
  const re = /translate\(\s*([-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?)(?:[\s,]+([-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?))?\s*\)/gi;
  let m;
  while ((m = re.exec(transformAttr)) !== null) {
    tx += Number(m[1]) || 0;
    ty += m[2] !== undefined ? Number(m[2]) || 0 : 0;
  }
  return { tx, ty };
}

/**
 * @param {Map<string, string>} attrs
 * @param {string} name
 * @returns {number | null}
 */
function getNumericAttr(attrs, name) {
  const raw = attrs.get(name);
  if (raw === undefined || raw === '') {
    return null;
  }
  const num = Number(raw);
  return Number.isFinite(num) ? num : null;
}

/**
 * @param {number} x
 * @param {number} y
 * @param {{ minX: number, minY: number, maxX: number, maxY: number }} bounds
 * @param {string} desc
 * @param {string[]} errors
 */
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

/**
 * Parse and bounds-check SVG `<path d="...">` coordinates across all SVG 1.1/2 path commands
 * (`M/m/L/l/H/h/V/v/C/c/S/s/Q/q/T/t/A/a/Z/z`), including `[eE]` scientific notation,
 * compact arc flags, subpath `Z` resets, and accumulated `translate(tx, ty)` offsets.
 *
 * @param {string} dAttr
 * @param {{ minX: number, minY: number, maxX: number, maxY: number }} bounds
 * @param {{ tx: number, ty: number }} offset
 * @param {string} label
 * @param {string[]} errors
 */
function checkPathBounds(dAttr, bounds, offset, label, errors) {
  const numRe = /^[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/;
  let pos = 0;
  const len = dAttr.length;
  let cmd = '';
  let curX = 0;
  let curY = 0;
  let startX = 0;
  let startY = 0;

  const skipSep = () => {
    while (pos < len && /[\s,]/.test(dAttr[pos])) {
      pos += 1;
    }
  };

  const peekIsCmd = () => {
    skipSep();
    return pos < len && /[A-Za-z]/.test(dAttr[pos]);
  };

  const readNum = () => {
    skipSep();
    if (pos >= len) return null;
    const m = numRe.exec(dAttr.slice(pos));
    if (!m) return null;
    pos += m[0].length;
    const n = Number(m[0]);
    return Number.isFinite(n) ? n : null;
  };

  const readFlag = () => {
    skipSep();
    if (pos >= len || (dAttr[pos] !== '0' && dAttr[pos] !== '1')) {
      return null;
    }
    const v = Number(dAttr[pos]);
    pos += 1;
    return v;
  };

  /** @param {number} x @param {number} y @param {string} tag */
  const checkPt = (x, y, tag) =>
    checkPointInBounds(x + offset.tx, y + offset.ty, bounds, `${label} <path> ${tag}`, errors);

  while (pos < len) {
    skipSep();
    if (pos >= len) break;

    if (/[A-Za-z]/.test(dAttr[pos])) {
      cmd = dAttr[pos];
      pos += 1;
      if (cmd === 'Z' || cmd === 'z') {
        curX = startX;
        curY = startY;
        cmd = '';
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
      if (cmd === 'M') {
        startX = curX;
        startY = curY;
        checkPt(curX, curY, 'M');
        cmd = 'L';
      } else {
        checkPt(curX, curY, cmd);
      }
    } else if (cmd === 'm' || cmd === 'l' || cmd === 't') {
      const dx = readNum();
      const dy = readNum();
      if (dx === null || dy === null) break;
      curX += dx;
      curY += dy;
      if (cmd === 'm') {
        startX = curX;
        startY = curY;
        checkPt(curX, curY, 'm');
        cmd = 'l';
      } else {
        checkPt(curX, curY, cmd);
      }
    } else if (cmd === 'H') {
      const x = readNum();
      if (x === null) break;
      curX = x;
      checkPt(curX, curY, 'H');
    } else if (cmd === 'h') {
      const dx = readNum();
      if (dx === null) break;
      curX += dx;
      checkPt(curX, curY, 'h');
    } else if (cmd === 'V') {
      const y = readNum();
      if (y === null) break;
      curY = y;
      checkPt(curX, curY, 'V');
    } else if (cmd === 'v') {
      const dy = readNum();
      if (dy === null) break;
      curY += dy;
      checkPt(curX, curY, 'v');
    } else if (cmd === 'C') {
      const x1 = readNum();
      const y1 = readNum();
      const x2 = readNum();
      const y2 = readNum();
      const x = readNum();
      const y = readNum();
      if (x1 === null || y1 === null || x2 === null || y2 === null || x === null || y === null) {
        break;
      }
      checkPt(x1, y1, 'C cp1');
      checkPt(x2, y2, 'C cp2');
      curX = x;
      curY = y;
      checkPt(curX, curY, 'C');
    } else if (cmd === 'c') {
      const dx1 = readNum();
      const dy1 = readNum();
      const dx2 = readNum();
      const dy2 = readNum();
      const dx = readNum();
      const dy = readNum();
      if (
        dx1 === null ||
        dy1 === null ||
        dx2 === null ||
        dy2 === null ||
        dx === null ||
        dy === null
      ) {
        break;
      }
      checkPt(curX + dx1, curY + dy1, 'c cp1');
      checkPt(curX + dx2, curY + dy2, 'c cp2');
      curX += dx;
      curY += dy;
      checkPt(curX, curY, 'c');
    } else if (cmd === 'S' || cmd === 'Q') {
      const x1 = readNum();
      const y1 = readNum();
      const x = readNum();
      const y = readNum();
      if (x1 === null || y1 === null || x === null || y === null) break;
      checkPt(x1, y1, `${cmd} cp`);
      curX = x;
      curY = y;
      checkPt(curX, curY, cmd);
    } else if (cmd === 's' || cmd === 'q') {
      const dx1 = readNum();
      const dy1 = readNum();
      const dx = readNum();
      const dy = readNum();
      if (dx1 === null || dy1 === null || dx === null || dy === null) break;
      checkPt(curX + dx1, curY + dy1, `${cmd} cp`);
      curX += dx;
      curY += dy;
      checkPt(curX, curY, cmd);
    } else if (cmd === 'A' || cmd === 'a') {
      const rx = readNum();
      const ry = readNum();
      const rot = readNum();
      const largeArc = readFlag();
      const sweep = readFlag();
      const x = readNum();
      const y = readNum();
      if (
        rx === null ||
        ry === null ||
        rot === null ||
        largeArc === null ||
        sweep === null ||
        x === null ||
        y === null
      ) {
        break;
      }
      if (cmd === 'A') {
        curX = x;
        curY = y;
      } else {
        curX += x;
        curY += y;
      }
      checkPt(curX, curY, cmd);
    } else {
      break;
    }

    if (peekIsCmd()) {
      continue;
    }
  }
}

/**
 * Find top-level balanced `<svg>...</svg>` elements in comment/script-free HTML.
 *
 * @param {string} html
 * @returns {Array<{ start: number, attrs: Map<string, string>, inner: string }>}
 */
function extractTopLevelSvgs(html) {
  /** @type {Array<{ start: number, attrs: Map<string, string>, inner: string }>} */
  const svgs = [];
  const tokenRe = new RegExp(`<(\\/)?svg\\b(${TAG_ATTRS_PATTERN})(\\/)?>`, 'gi');
  let depth = 0;
  let openStart = -1;
  let innerStart = -1;
  /** @type {Map<string, string> | null} */
  let openAttrs = null;

  let m;
  while ((m = tokenRe.exec(html)) !== null) {
    const isClosing = Boolean(m[1]);
    const isSelfClosing = Boolean(m[3]);
    if (!isClosing) {
      if (depth === 0) {
        openStart = m.index;
        openAttrs = parseTagAttrs(m[2]);
        innerStart = m.index + m[0].length;
      }
      if (isSelfClosing) {
        if (depth === 0 && openAttrs) {
          svgs.push({ start: openStart, attrs: openAttrs, inner: '' });
        }
      } else {
        depth += 1;
      }
    } else if (depth > 0) {
      depth -= 1;
      if (depth === 0 && openAttrs) {
        svgs.push({
          start: openStart,
          attrs: openAttrs,
          inner: html.slice(innerStart, m.index),
        });
        openAttrs = null;
      }
    }
  }
  return svgs;
}

/**
 * Validate inline SVG diagrams in an HTML document.
 *
 * Targets every `<svg>` inside `<figure>`, carrying `role="img"`, or with `viewBox` width/height >= 200
 * (so decorative 16-32px UI icons remain exempt while forgotten diagram attributes are caught).
 *
 * @param {string} html
 * @param {string} relFile
 * @returns {{ diagramCount: number, errors: string[] }}
 */
export function validateSvgDiagramsInHtml(html, relFile) {
  const clean = stripHtmlNoise(html);
  const docIds = extractElementIds(clean);
  /** @type {string[]} */
  const errors = [];
  let diagramCount = 0;

  /** @type {Array<[number, number]>} */
  const figureRanges = [];
  const figRe = new RegExp(`<figure\\b${TAG_ATTRS_PATTERN}>[\\s\\S]*?<\\/figure\\s*>`, 'gi');
  let figMatch;
  while ((figMatch = figRe.exec(clean)) !== null) {
    figureRanges.push([figMatch.index, figMatch.index + figMatch[0].length]);
  }
  /** @param {number} pos */
  const inFigure = (pos) => figureRanges.some(([start, end]) => pos >= start && pos < end);

  for (const svg of extractTopLevelSvgs(clean)) {
    const { attrs, inner } = svg;
    const role = attrs.get('role');
    const ariaHidden = attrs.get('aria-hidden');
    const viewBoxRaw = attrs.get('viewbox') ?? null;
    const viewBoxParts = viewBoxRaw ? viewBoxRaw.trim().split(/[\s,]+/).map(Number) : null;
    const isLargeSvg =
      viewBoxParts !== null &&
      viewBoxParts.length === 4 &&
      Number.isFinite(viewBoxParts[2]) &&
      Number.isFinite(viewBoxParts[3]) &&
      (viewBoxParts[2] >= 200 || viewBoxParts[3] >= 200);

    const isDiagram =
      role === 'img' || (inFigure(svg.start) && ariaHidden !== 'true') || isLargeSvg;
    if (!isDiagram) {
      continue;
    }

    diagramCount += 1;
    const label = `${relFile} SVG diagram #${diagramCount}`;

    if (role !== 'img') {
      errors.push(`${label}: missing role="img"`);
    }
    if (ariaHidden === 'true') {
      errors.push(`${label}: diagram carries contradictory aria-hidden="true"`);
    }

    const labelledBy = attrs.get('aria-labelledby');
    const innerIds = extractElementIds(inner);
    if (!labelledBy || !labelledBy.trim()) {
      errors.push(`${label}: missing aria-labelledby attribute`);
    } else {
      for (const refId of labelledBy.trim().split(/\s+/)) {
        if (!innerIds.has(refId) && !docIds.has(refId)) {
          errors.push(`${label}: aria-labelledby references missing id "${refId}"`);
        }
      }
    }

    const titleMatch = new RegExp(`<title\\b${TAG_ATTRS_PATTERN}>([\\s\\S]*?)<\\/title\\s*>`, 'i').exec(inner);
    if (!titleMatch || !titleMatch[1].trim()) {
      errors.push(`${label}: missing non-empty <title> element`);
    }
    const descMatch = new RegExp(`<desc\\b${TAG_ATTRS_PATTERN}>([\\s\\S]*?)<\\/desc\\s*>`, 'i').exec(inner);
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

    // Strip non-rendered definition containers whose coordinates live in local spaces.
    let body = inner;
    for (const defTag of ['defs', 'marker', 'symbol', 'pattern', 'clippath', 'mask']) {
      body = stripPairedTagBlocks(body, defTag);
    }

    // Walk tags in order, tracking nested `<g transform="translate(...)">` scopes.
    /** @type {Array<{ tx: number, ty: number }>} */
    const translateStack = [{ tx: 0, ty: 0 }];
    const currentOffset = () => translateStack[translateStack.length - 1];

    const elemRe = new RegExp(`<(\\/)?([a-z][a-z0-9-]*)\\b(${TAG_ATTRS_PATTERN})(\\/)?>`, 'gi');
    let elemMatch;
    while ((elemMatch = elemRe.exec(body)) !== null) {
      const isClose = Boolean(elemMatch[1]);
      const tag = elemMatch[2].toLowerCase();
      const selfClose = Boolean(elemMatch[4]);

      if (tag === 'g') {
        if (isClose) {
          if (translateStack.length > 1) {
            translateStack.pop();
          }
        } else if (!selfClose) {
          const gAttrs = parseTagAttrs(elemMatch[3]);
          const delta = parseTranslate(gAttrs.get('transform'));
          const prev = currentOffset();
          translateStack.push({ tx: prev.tx + delta.tx, ty: prev.ty + delta.ty });
        }
        continue;
      }
      if (isClose) continue;

      const a = parseTagAttrs(elemMatch[3]);
      const local = parseTranslate(a.get('transform'));
      const base = currentOffset();
      const off = { tx: base.tx + local.tx, ty: base.ty + local.ty };

      if (tag === 'rect') {
        const x = getNumericAttr(a, 'x') ?? 0;
        const y = getNumericAttr(a, 'y') ?? 0;
        const w = getNumericAttr(a, 'width');
        const h = getNumericAttr(a, 'height');
        if (w !== null && w < 0) errors.push(`${label} <rect> has negative width (${w})`);
        if (h !== null && h < 0) errors.push(`${label} <rect> has negative height (${h})`);
        if (w !== null && h !== null && w >= 0 && h >= 0) {
          checkPointInBounds(x + off.tx, y + off.ty, bounds, `${label} <rect> top-left`, errors);
          checkPointInBounds(
            x + w + off.tx,
            y + h + off.ty,
            bounds,
            `${label} <rect> bottom-right`,
            errors,
          );
        }
      } else if (tag === 'circle') {
        const cx = getNumericAttr(a, 'cx') ?? 0;
        const cy = getNumericAttr(a, 'cy') ?? 0;
        const r = getNumericAttr(a, 'r') ?? 0;
        if (r < 0) {
          errors.push(`${label} <circle> has negative radius (${r})`);
        } else {
          checkPointInBounds(cx - r + off.tx, cy - r + off.ty, bounds, `${label} <circle> min`, errors);
          checkPointInBounds(cx + r + off.tx, cy + r + off.ty, bounds, `${label} <circle> max`, errors);
        }
      } else if (tag === 'ellipse') {
        const cx = getNumericAttr(a, 'cx') ?? 0;
        const cy = getNumericAttr(a, 'cy') ?? 0;
        const rx = getNumericAttr(a, 'rx') ?? 0;
        const ry = getNumericAttr(a, 'ry') ?? 0;
        if (rx < 0 || ry < 0) {
          errors.push(`${label} <ellipse> has negative radius (${rx}, ${ry})`);
        } else {
          checkPointInBounds(cx - rx + off.tx, cy - ry + off.ty, bounds, `${label} <ellipse> min`, errors);
          checkPointInBounds(cx + rx + off.tx, cy + ry + off.ty, bounds, `${label} <ellipse> max`, errors);
        }
      } else if (tag === 'line') {
        const x1 = getNumericAttr(a, 'x1');
        const y1 = getNumericAttr(a, 'y1');
        const x2 = getNumericAttr(a, 'x2');
        const y2 = getNumericAttr(a, 'y2');
        if (x1 !== null && y1 !== null) {
          checkPointInBounds(x1 + off.tx, y1 + off.ty, bounds, `${label} <line> start`, errors);
        }
        if (x2 !== null && y2 !== null) {
          checkPointInBounds(x2 + off.tx, y2 + off.ty, bounds, `${label} <line> end`, errors);
        }
      } else if (tag === 'text') {
        const x = getNumericAttr(a, 'x');
        const y = getNumericAttr(a, 'y');
        if (x !== null && y !== null) {
          checkPointInBounds(x + off.tx, y + off.ty, bounds, `${label} <text> anchor`, errors);
        }
      } else if (tag === 'polygon' || tag === 'polyline') {
        const ptsAttr = a.get('points');
        if (ptsAttr) {
          const nums = (ptsAttr.match(/[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/g) ?? []).map(Number);
          for (let i = 0; i + 1 < nums.length; i += 2) {
            checkPointInBounds(
              nums[i] + off.tx,
              nums[i + 1] + off.ty,
              bounds,
              `${label} <${tag}> point`,
              errors,
            );
          }
        }
      } else if (tag === 'path') {
        const dAttr = a.get('d');
        if (dAttr) {
          checkPathBounds(dAttr, bounds, off, label, errors);
        }
      }
    }
  }

  return { diagramCount, errors };
}

/**
 * @param {string} distDir
 * @returns {string[]}
 */
function collectHtmlFiles(distDir) {
  /** @type {string[]} */
  const out = [];
  /** @param {string} dir */
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

/**
 * Read `[workspace.package].version` strictly from root `Cargo.toml`.
 *
 * @param {string} repoRoot
 * @returns {string | null}
 */
function readWorkspaceVersion(repoRoot) {
  const cargoTomlPath = join(repoRoot, 'Cargo.toml');
  if (!existsSync(cargoTomlPath) || !statSync(cargoTomlPath).isFile()) {
    return null;
  }
  const raw = readFileSync(cargoTomlPath, 'utf8').replace(/\r\n/g, '\n');
  const sectionMatch = /^\[workspace\.package\]\s*$([\s\S]*?)(?=^\[|\Z)/m.exec(raw);
  const section =
    sectionMatch?.[1] ??
    /\[workspace\.package\]([\s\S]*?)(?=\n\[|$)/.exec(raw)?.[1] ??
    null;
  if (!section) {
    return null;
  }
  return /^\s*version\s*=\s*"([^"]+)"/m.exec(section)?.[1] ?? null;
}

/**
 * Parse every `<div id="tour-panel-...">...</div>` container in `index.html` using balanced
 * quote-aware `<div>` depth tracking (resilient to nested divs, quotes, and sibling markup).
 *
 * @param {string} indexHtml
 * @returns {{ totalPanelIds: number, panels: Array<{ id: string, sourceRelPath: string, code: string }> }}
 */
function parseOutputTourPanels(indexHtml) {
  const clean = stripHtmlNoise(indexHtml);
  const divTokenRe = new RegExp(`<(\\/)?div\\b(${TAG_ATTRS_PATTERN})(\\/)?>`, 'gi');
  /** @type {Array<{ id: string, sourceRelPath: string, code: string }>} */
  const panels = [];
  let totalPanelIds = 0;

  let trackingId = '';
  let trackingDepth = 0;
  let innerStart = -1;

  let m;
  while ((m = divTokenRe.exec(clean)) !== null) {
    const isClose = Boolean(m[1]);
    const isSelfClose = Boolean(m[3]);

    if (!trackingId) {
      if (!isClose && !isSelfClose) {
        const attrs = parseTagAttrs(m[2]);
        const id = attrs.get('id') ?? '';
        if (id.startsWith('tour-panel-')) {
          totalPanelIds += 1;
          trackingId = id.slice('tour-panel-'.length);
          trackingDepth = 1;
          innerStart = m.index + m[0].length;
        }
      }
    } else if (!isClose) {
      if (!isSelfClose) {
        trackingDepth += 1;
      }
    } else {
      trackingDepth -= 1;
      if (trackingDepth === 0) {
        const body = clean.slice(innerStart, m.index);
        const panelLinks = extractLinks(body);
        const sourceLink = panelLinks.find(
          (l) =>
            l.attr === 'href' &&
            l.value.startsWith(`${REPO_GITHUB_PREFIX}blob/main/examples/`),
        );
        const preMatch = new RegExp(`<pre\\b${TAG_ATTRS_PATTERN}>([\\s\\S]*?)<\\/pre\\s*>`, 'i').exec(body);
        if (sourceLink && preMatch) {
          const sourceRelPath = sourceLink.value
            .slice(`${REPO_GITHUB_PREFIX}blob/main/`.length)
            .split(/[?#]/)[0];
          const code = decodeHtmlEntities(preMatch[1].replace(ANY_HTML_TAG_RE, ''))
            .replace(/\r\n/g, '\n')
            .trimEnd();
          panels.push({ id: trackingId, sourceRelPath, code });
        }
        trackingId = '';
        innerStart = -1;
      }
    }
  }

  return { totalPanelIds, panels };
}

/**
 * Check whether `excerpt` matches a contiguous slice of complete lines in `source`.
 *
 * @param {string} source
 * @param {string} excerpt
 * @returns {boolean}
 */
function containsCompleteLines(source, excerpt) {
  if (!excerpt.trim()) {
    return false;
  }
  const srcLines = source.replace(/\r\n/g, '\n').split('\n');
  const exLines = excerpt.replace(/\r\n/g, '\n').split('\n');
  if (exLines.length === 0 || exLines.length > srcLines.length) {
    return false;
  }
  outer: for (let i = 0; i <= srcLines.length - exLines.length; i += 1) {
    for (let j = 0; j < exLines.length; j += 1) {
      if (srcLines[i + j] !== exLines[j]) {
        continue outer;
      }
    }
    return true;
  }
  return false;
}

/**
 * Strip fenced code blocks from Markdown/plain text while preserving line structure.
 *
 * @param {string} text
 * @returns {string}
 */
function stripFencedCodeBlocks(text) {
  const lines = String(text).replace(/\r\n/g, '\n').split('\n');
  let inFence = false;
  let fenceChar = '';
  let fenceLen = 0;
  return lines
    .map((line) => {
      const trimmed = line.trimStart();
      if (!inFence) {
        const m = /^(`{3,})[^`]*$/.exec(trimmed) || /^(~{3,})/.exec(trimmed);
        if (m) {
          inFence = true;
          fenceChar = m[1][0];
          fenceLen = m[1].length;
          return '';
        }
        return line;
      }
      if (new RegExp(`^${fenceChar}{${fenceLen},}\\s*$`).test(trimmed)) {
        inFence = false;
      }
      return '';
    })
    .join('\n');
}

/**
 * Verify a `https://github.com/asamarts/witforge/(blob|tree|edit)/main/...` URL against `repoRoot`.
 *
 * @param {string} url
 * @param {string} sourceLabel
 * @param {string} repoRoot
 * @param {string[]} errors
 * @returns {boolean}
 */
function verifyGitHubRepoUrl(url, sourceLabel, repoRoot, errors) {
  if (!url.startsWith(REPO_GITHUB_PREFIX)) {
    return false;
  }
  const rest = url.slice(REPO_GITHUB_PREFIX.length);
  const match = /^(blob|tree|edit)\/main\/([^?#]*)/.exec(rest);
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
  if (!relPath || relPath.includes('\\')) {
    errors.push(`${sourceLabel}: invalid or empty GitHub repo path in ${url}`);
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
  try {
    const realRepo = realpathSync(absRepo);
    const realTarget = realpathSync(target);
    const realRel = toPosix(relative(realRepo, realTarget));
    if (!realRel || isAbsolute(realRel) || realRel === '..' || realRel.startsWith('../')) {
      errors.push(`${sourceLabel}: GitHub repo link symlink escapes repository root: ${url}`);
      return true;
    }
  } catch {
    errors.push(`${sourceLabel}: unable to resolve real path for GitHub repo link: ${url}`);
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
  /** @type {string[]} */
  const errors = [];
  const emptyStats = {
    canonicalHtmlPages: 0,
    redirectStubs: 0,
    checkedInternalLinks: 0,
    checkedFragments: 0,
    verifiedDiagnosticCodes: 0,
    checkedRepoLinks: 0,
    checkedDiagrams: 0,
    verifiedTourPanels: 0,
    workspaceVersion: /** @type {string | null} */ (null),
  };

  const absDist = resolve(distDir);
  if (!existsSync(absDist) || !statSync(absDist).isDirectory()) {
    return {
      errors: [`Built dist directory not found: ${absDist}`],
      stats: emptyStats,
    };
  }

  const normalizedOrigin = siteOrigin.replace(/\/+$/, '');
  const normalizedBase = basePath === '/' ? '' : basePath.replace(/\/+$/, '');
  const siteBaseUrl = `${normalizedOrigin}${normalizedBase}`;

  // Load redirect manifest dynamically so redirect stubs are exempt from canonical trailing-slash checks.
  /** @type {Array<{ from: string, to: string, title: string }>} */
  let redirectEntries = [];
  /** @type {Set<string>} */
  const redirectSourceSet = new Set();
  /** @type {Map<string, string>} */
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
      errors.push(`Redirect manifest error: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  const allHtmlFiles = collectHtmlFiles(absDist);
  const canonicalHtmlFiles = allHtmlFiles.filter((rel) => !redirectSourceSet.has(rel));
  /** @type {Map<string, string>} */
  const htmlCache = new Map();
  /** @type {Map<string, Set<string>>} */
  const idsByFile = new Map();

  for (const rel of allHtmlFiles) {
    const raw = readFileSync(join(absDist, ...rel.split('/')), 'utf8');
    htmlCache.set(rel, raw);
    idsByFile.set(rel, extractElementIds(raw));
  }

  let checkedInternalLinks = 0;
  let checkedFragments = 0;
  let checkedRepoLinks = 0;
  let checkedDiagrams = 0;
  let verifiedDiagnosticCodes = 0;
  let verifiedTourPanels = 0;
  /** @type {string | null} */
  let workspaceVersion = null;

  /**
   * Verify a resolved dist path stays strictly inside `absDist` and exists as a regular file.
   *
   * @param {string} distRel
   * @returns {boolean}
   */
  const distFileExists = (distRel) => {
    const segments = distRel.split('/').filter(Boolean);
    if (segments.some((s) => s === '.' || s === '..')) {
      return false;
    }
    const resolved = resolve(absDist, ...segments);
    const relCheck = toPosix(relative(absDist, resolved));
    if (!relCheck || isAbsolute(relCheck) || relCheck === '..' || relCheck.startsWith('../')) {
      return false;
    }
    return existsSync(resolved) && statSync(resolved).isFile();
  };

  /**
   * Resolve an internal route/path to a relative file in distDir, enforcing trailing slashes on pages
   * and rejecting `..` escapes, duplicate slashes, and canonical links to stubs/`404.html`/`index.html`.
   *
   * @param {string} rawHref
   * @param {string} currentFile
   * @param {boolean} isRedirectStub
   * @returns {{ targetFile: string, fragment: string } | null}
   */
  const resolveInternalTarget = (rawHref, currentFile, isRedirectStub) => {
    let urlPath = rawHref;
    if (/^https?:\/\//i.test(urlPath)) {
      let parsed;
      try {
        parsed = new URL(urlPath);
      } catch {
        errors.push(`${currentFile}: malformed URL "${rawHref}"`);
        return null;
      }
      if (parsed.origin !== normalizedOrigin) {
        return null; // External origin
      }
      // Special case: bare origin without trailing slash when basePath === '/' (e.g. `https://example.com#frag`).
      const rawAfterOrigin = urlPath.slice(parsed.origin.length);
      if (!normalizedBase && !rawAfterOrigin.startsWith('/')) {
        errors.push(`${currentFile}: internal link missing trailing slash: "${rawHref}"`);
        if (!distFileExists('index.html')) {
          errors.push(`${currentFile}: broken internal link "${rawHref}" (missing index.html)`);
          return null;
        }
        return { targetFile: 'index.html', fragment: parsed.hash.replace(/^#/, '') };
      }
      urlPath = `${parsed.pathname}${parsed.search}${parsed.hash}`;
    } else if (urlPath.startsWith('//')) {
      return null;
    }

    const hashIdx = urlPath.indexOf('#');
    const fragment = hashIdx === -1 ? '' : urlPath.slice(hashIdx + 1);
    const beforeHash = hashIdx === -1 ? urlPath : urlPath.slice(0, hashIdx);
    const queryIdx = beforeHash.indexOf('?');
    const rawPathOnly = queryIdx === -1 ? beforeHash : beforeHash.slice(0, queryIdx);

    if (!rawPathOnly) {
      return { targetFile: currentFile, fragment };
    }

    let pathOnly;
    try {
      pathOnly = decodeURIComponent(rawPathOnly);
    } catch {
      errors.push(`${currentFile}: malformed percent-encoding in internal link "${rawHref}"`);
      return null;
    }

    if (pathOnly.includes('\\')) {
      errors.push(`${currentFile}: backslash in internal link "${rawHref}"`);
      return null;
    }

    let distRelPath;
    if (pathOnly.startsWith('/')) {
      if (pathOnly.includes('//')) {
        errors.push(`${currentFile}: duplicate slash in internal link "${rawHref}"`);
        return null;
      }
      const rawParts = pathOnly.split('/').filter(Boolean);
      if (rawParts.some((seg) => seg === '.' || seg === '..')) {
        errors.push(`${currentFile}: root-relative link contains dot segment: "${rawHref}"`);
        return null;
      }
      if (normalizedBase) {
        if (pathOnly === normalizedBase) {
          errors.push(`${currentFile}: internal link missing trailing slash: "${rawHref}"`);
          if (!distFileExists('index.html')) {
            errors.push(`${currentFile}: broken internal link "${rawHref}" (missing index.html)`);
            return null;
          }
          return { targetFile: 'index.html', fragment };
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
      distRelPath = distRelPath.replace(/^\.\/?/, '');
    }

    // Empty means site root `/` -> `index.html`.
    if (!distRelPath) {
      if (!distFileExists('index.html')) {
        errors.push(`${currentFile}: broken internal link "${rawHref}" (missing index.html)`);
        return null;
      }
      return { targetFile: 'index.html', fragment };
    }

    if (distRelPath.endsWith('/')) {
      const candidate = `${distRelPath}index.html`;
      if (!distFileExists(candidate)) {
        errors.push(`${currentFile}: broken internal link "${rawHref}" (missing ${candidate})`);
        return null;
      }
      return { targetFile: candidate, fragment };
    }

    const baseName = posix.basename(distRelPath);
    const hasExtension = baseName.includes('.');
    if (hasExtension) {
      if (!distFileExists(distRelPath)) {
        errors.push(`${currentFile}: broken internal asset/file link "${rawHref}" (missing ${distRelPath})`);
        return null;
      }
      if (!isRedirectStub) {
        if (redirectSourceSet.has(distRelPath)) {
          errors.push(
            `${currentFile}: canonical page links to legacy redirect stub "${rawHref}" instead of "${redirectTargetMap.get(distRelPath)}"`,
          );
        } else if (distRelPath === '404.html') {
          errors.push(`${currentFile}: canonical page links directly to 404.html ("${rawHref}")`);
        } else if (baseName === 'index.html') {
          errors.push(`${currentFile}: canonical page links to explicit index.html ("${rawHref}") instead of trailing-slash route`);
        }
      }
      return { targetFile: distRelPath, fragment };
    }

    const dirIndex = `${distRelPath}/index.html`;
    if (distFileExists(dirIndex)) {
      errors.push(`${currentFile}: canonical page link missing trailing slash: "${rawHref}"`);
      return { targetFile: dirIndex, fragment };
    }

    errors.push(`${currentFile}: broken internal link "${rawHref}" (missing ${distRelPath})`);
    return null;
  };

  /**
   * Verify a `#fragment` on a resolved internal link against `idsByFile` (following redirect stubs).
   *
   * @param {{ targetFile: string, fragment: string }} resolved
   * @param {string} rawHref
   * @param {string} sourceLabel
   */
  const verifyResolvedFragment = (resolved, rawHref, sourceLabel) => {
    if (!resolved.fragment) {
      return;
    }
    checkedFragments += 1;
    let fragTargetFile = resolved.targetFile;
    if (redirectTargetMap.has(fragTargetFile)) {
      const destRoute = redirectTargetMap.get(fragTargetFile) ?? '';
      const sub = destRoute.slice(normalizedBase.length).replace(/^\/+|\/+$/g, '');
      fragTargetFile = sub ? `${sub}/index.html` : 'index.html';
    }
    const targetIds = idsByFile.get(fragTargetFile);
    if (!targetIds) {
      errors.push(
        `${sourceLabel}: fragment link "${rawHref}" targets non-HTML or missing page "${fragTargetFile}"`,
      );
      return;
    }
    let decodedFrag = resolved.fragment;
    try {
      decodedFrag = decodeURIComponent(resolved.fragment);
    } catch {
      // Keep raw fragment for comparison
    }
    if (!targetIds.has(decodedFrag) && !targetIds.has(resolved.fragment)) {
      errors.push(
        `${sourceLabel}: broken fragment "#${resolved.fragment}" in "${rawHref}" (not found in ${fragTargetFile})`,
      );
    }
  };

  // Checks 1, 2 (fragments), 4 (GitHub links), 6 (SVG diagrams), and 7 (prose/em-dash/cliché) across HTML.
  for (const rel of allHtmlFiles) {
    const raw = htmlCache.get(rel) ?? '';
    const isRedirectStub = redirectSourceSet.has(rel);

    for (const issue of findProseIssuesInHtml(raw)) {
      errors.push(`${rel}: ${issue}`);
    }

    if (!isRedirectStub) {
      const svgResult = validateSvgDiagramsInHtml(raw, rel);
      checkedDiagrams += svgResult.diagramCount;
      errors.push(...svgResult.errors);
    }

    const links = extractLinks(raw);
    for (const link of links) {
      if (/^javascript:/i.test(link.value)) {
        errors.push(`${rel}: disallowed javascript: URL in ${link.attr}: "${link.value}"`);
        continue;
      }
      if (URI_SCHEME_RE.test(link.value) && !/^https?:/i.test(link.value)) {
        continue;
      }

      if (verifyGitHubRepoUrl(link.value, rel, repoRoot, errors)) {
        checkedRepoLinks += 1;
        continue;
      }

      const resolved = resolveInternalTarget(link.value, rel, isRedirectStub);
      if (!resolved) {
        continue;
      }
      checkedInternalLinks += 1;
      verifyResolvedFragment(resolved, link.value, rel);
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
    if (!existsSync(diagMdPath) || !statSync(diagMdPath).isFile()) {
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
        if (!diagIds?.has(lower)) {
          errors.push(
            `${diagHtmlRel}: missing lowercase anchor id="${lower}" for HELP_BASE_URL code ${code}`,
          );
        } else {
          verifiedDiagnosticCodes += 1;
        }
      }
    }
  }

  // Collect sitemap chunk paths from sitemap-index.xml for both Check 3 and Check 5.
  /** @type {string[]} */
  const discoveredSitemapChunks = [];
  const sitemapIndexPath = join(absDist, 'sitemap-index.xml');
  if (existsSync(sitemapIndexPath) && statSync(sitemapIndexPath).isFile()) {
    const idxXml = readFileSync(sitemapIndexPath, 'utf8');
    for (const m of idxXml.matchAll(/<loc>([^<]+)<\/loc>/gi)) {
      const loc = decodeHtmlEntities(m[1].trim());
      if (loc.startsWith(`${siteBaseUrl}/`)) {
        const subRel = loc.slice(`${siteBaseUrl}/`.length);
        if (subRel && !subRel.includes('..') && !subRel.includes('\\')) {
          discoveredSitemapChunks.push(subRel);
        }
      }
    }
  }
  if (discoveredSitemapChunks.length === 0 && distFileExists('sitemap-0.xml')) {
    discoveredSitemapChunks.push('sitemap-0.xml');
  }

  // Check 3: Legacy redirect stubs integrity (including exact `var target` match) & sitemap exclusion.
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
      const expectedJsTarget = `var target = ${escapeJsString(entry.to)};`;
      if (!stubHtml.includes(expectedJsTarget)) {
        errors.push(`${stubRel}: missing exact JS redirect target (${expectedJsTarget})`);
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

    for (const chunkRel of discoveredSitemapChunks) {
      if (!distFileExists(chunkRel)) continue;
      const sitemapXml = readFileSync(join(absDist, ...chunkRel.split('/')), 'utf8');
      for (const entry of redirectEntries) {
        const legacyLoc = `${normalizedOrigin}${normalizedBase}${entry.from}`;
        if (sitemapXml.includes(legacyLoc)) {
          errors.push(`${chunkRel} must not include legacy redirect stub: ${legacyLoc}`);
        }
      }
    }
  }

  // Check 5 & 7b: Required site artifacts + sitemap integrity + Pagefind entry JSON + llms*.txt checks.
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
    if (existsSync(robotsPath) && statSync(robotsPath).isFile()) {
      const robotsTxt = readFileSync(robotsPath, 'utf8');
      const sitemapDirectives = [...robotsTxt.matchAll(/^\s*sitemap\s*:\s*(\S+)\s*$/gim)].map(
        (m) => m[1].trim(),
      );
      if (sitemapDirectives.length === 0) {
        errors.push('robots.txt does not declare a Sitemap directive');
      }
      for (const directive of sitemapDirectives) {
        if (!directive.startsWith(`${siteBaseUrl}/`)) {
          errors.push(`robots.txt Sitemap URL does not match site base "${siteBaseUrl}/": ${directive}`);
          continue;
        }
        const subRel = directive.slice(`${siteBaseUrl}/`.length);
        if (!distFileExists(subRel) || statSync(join(absDist, ...subRel.split('/'))).size === 0) {
          errors.push(`robots.txt references missing or empty sitemap file: ${subRel}`);
          continue;
        }
        if (subRel === 'sitemap-index.xml') {
          const idxXml = readFileSync(join(absDist, 'sitemap-index.xml'), 'utf8');
          const locs = [...idxXml.matchAll(/<loc>([^<]+)<\/loc>/gi)].map((m) =>
            decodeHtmlEntities(m[1].trim()),
          );
          if (locs.length === 0) {
            errors.push('sitemap-index.xml contains no <loc> entries');
          }
          for (const loc of locs) {
            if (!loc.startsWith(`${siteBaseUrl}/`)) {
              errors.push(`sitemap-index.xml <loc> has wrong origin/base: ${loc}`);
              continue;
            }
            const chunkRel = loc.slice(`${siteBaseUrl}/`.length);
            if (
              !distFileExists(chunkRel) ||
              statSync(join(absDist, ...chunkRel.split('/'))).size === 0
            ) {
              errors.push(`sitemap-index.xml references missing or empty sitemap file: ${chunkRel}`);
              continue;
            }
            // Validate every <loc> URL inside each sitemap chunk resolves to a canonical built page.
            const chunkXml = readFileSync(join(absDist, ...chunkRel.split('/')), 'utf8');
            const chunkLocs = [...chunkXml.matchAll(/<loc>([^<]+)<\/loc>/gi)].map((m) =>
              decodeHtmlEntities(m[1].trim()),
            );
            if (chunkLocs.length === 0) {
              errors.push(`${chunkRel} contains no <loc> URLs`);
            }
            for (const pageLoc of chunkLocs) {
              resolveInternalTarget(pageLoc, chunkRel, false);
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
        if (!existsSync(pfFile) || !statSync(pfFile).isFile() || statSync(pfFile).size === 0) {
          errors.push(`Missing or empty Pagefind artifact: pagefind/${pfReq}`);
        }
      }
      const entryPath = join(pagefindDir, 'pagefind-entry.json');
      if (existsSync(entryPath) && statSync(entryPath).isFile()) {
        try {
          /** @type {unknown} */
          const parsed = JSON.parse(readFileSync(entryPath, 'utf8'));
          const languages =
            parsed && typeof parsed === 'object' && 'languages' in parsed
              ? /** @type {{ languages?: unknown }} */ (parsed).languages
              : null;
          const totalPages =
            languages && typeof languages === 'object'
              ? Object.values(/** @type {Record<string, unknown>} */ (languages)).reduce(
                  (/** @type {number} */ sum, lang) => {
                    const count =
                      lang && typeof lang === 'object' && 'page_count' in lang
                        ? /** @type {{ page_count?: unknown }} */ (lang).page_count
                        : undefined;
                    return sum + (typeof count === 'number' ? count : 0);
                  },
                  0,
                )
              : 0;
          if (totalPages <= 0) {
            errors.push('pagefind/pagefind-entry.json reports zero indexed pages');
          }
        } catch (err) {
          errors.push(
            `pagefind/pagefind-entry.json is invalid JSON: ${err instanceof Error ? err.message : String(err)}`,
          );
        }
      }
      const fragDir = join(pagefindDir, 'fragment');
      if (!existsSync(fragDir) || !statSync(fragDir).isDirectory()) {
        errors.push('Missing Pagefind fragment directory: pagefind/fragment/');
      } else {
        const fragments = readdirSync(fragDir).filter((name) => name.endsWith('.pf_fragment'));
        if (fragments.length === 0) {
          errors.push('Pagefind index contains no .pf_fragment files under pagefind/fragment/');
        }
      }
    }

    // Check llms.txt & llms-full.txt prose hygiene, site URLs (+ fragments!), and GitHub links.
    const escapedSiteBase = siteBaseUrl.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const siteUrlRe = new RegExp(
      `${escapedSiteBase}(?=$|[/?#\\s)>'"\`\\].,;:])(?:[/?#][^\\s)>'"\`\\]]*)?`,
      'g',
    );
    const ghUrlRe = /https:\/\/github\.com\/asamarts\/witforge\/(?:blob|tree|edit)\/main\/[^\s)>'"`\]]*/g;

    for (const llmsRel of ['llms.txt', 'llms-full.txt']) {
      const llmsPath = join(absDist, llmsRel);
      if (existsSync(llmsPath) && statSync(llmsPath).isFile()) {
        const text = readFileSync(llmsPath, 'utf8');
        for (const issue of findProseIssuesInPlainText(text)) {
          errors.push(`${llmsRel}: ${issue}`);
        }
        const outsideFences = stripFencedCodeBlocks(text);
        for (const m of outsideFences.matchAll(ghUrlRe)) {
          const url = m[0].replace(/[.,;:]+$/, '');
          if (verifyGitHubRepoUrl(url, llmsRel, repoRoot, errors)) {
            checkedRepoLinks += 1;
          }
        }
        for (const m of outsideFences.matchAll(siteUrlRe)) {
          const url = m[0].replace(/[.,;:]+$/, '');
          const resolved = resolveInternalTarget(url, llmsRel, false);
          if (resolved) {
            checkedInternalLinks += 1;
            verifyResolvedFragment(resolved, url, llmsRel);
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
      const verEscaped = workspaceVersion.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      const vTokenRe = new RegExp(`\\bv${verEscaped}(?![0-9A-Za-z._-])`);
      const bareVerRe = new RegExp(`(?<![0-9A-Za-z._-])v?${verEscaped}(?![0-9A-Za-z._-])`);

      const indexHtml = htmlCache.get('index.html');
      const indexVisible = indexHtml
        ? decodeHtmlEntities(stripHtmlNoise(indexHtml).replace(ANY_HTML_TAG_RE, ' '))
        : '';
      if (!indexHtml || !vTokenRe.test(indexVisible)) {
        errors.push(`index.html does not reflect live workspace version v${workspaceVersion}`);
      }
      for (const llmsRel of ['llms.txt', 'llms-full.txt']) {
        const llmsPath = join(absDist, llmsRel);
        if (existsSync(llmsPath) && statSync(llmsPath).isFile()) {
          const llmsText = readFileSync(llmsPath, 'utf8');
          const headerBlock = llmsText.split('\n').slice(0, 15).join('\n');
          if (!bareVerRe.test(headerBlock)) {
            errors.push(`${llmsRel} header does not reflect live workspace version ${workspaceVersion}`);
          }
        }
      }
    }

    const indexHtml = htmlCache.get('index.html');
    if (!indexHtml) {
      errors.push('Missing dist/index.html for Output Tour example verification');
    } else {
      const { totalPanelIds, panels } = parseOutputTourPanels(indexHtml);
      if (panels.length === 0) {
        errors.push('index.html contains no verifiable Output Tour code panels (tour-panel-*)');
      } else if (panels.length !== totalPanelIds) {
        errors.push(
          `index.html declares ${totalPanelIds} tour-panel-* containers but only ${panels.length} had a verifiable source link and code block`,
        );
      }

      let sawSingle = false;
      let sawEvolved = false;
      const absExamples = resolve(repoRoot, 'examples');
      for (const panel of panels) {
        if (panel.sourceRelPath.startsWith('examples/single/')) sawSingle = true;
        if (panel.sourceRelPath.startsWith('examples/evolved/')) sawEvolved = true;
        const segments = panel.sourceRelPath.split('/').filter(Boolean);
        if (segments.some((s) => s === '.' || s === '..')) {
          errors.push(`Output Tour panel "${panel.id}" has unsafe example path: ${panel.sourceRelPath}`);
          continue;
        }
        const exampleFile = resolve(repoRoot, ...segments);
        const relEx = toPosix(relative(absExamples, exampleFile));
        if (!relEx || isAbsolute(relEx) || relEx === '..' || relEx.startsWith('../')) {
          errors.push(`Output Tour panel "${panel.id}" escapes examples/: ${panel.sourceRelPath}`);
          continue;
        }
        if (!existsSync(exampleFile) || !statSync(exampleFile).isFile()) {
          errors.push(
            `Output Tour panel "${panel.id}" references missing example file: ${panel.sourceRelPath}`,
          );
          continue;
        }
        const sourceRaw = readFileSync(exampleFile, 'utf8');
        if (!containsCompleteLines(sourceRaw, panel.code)) {
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
