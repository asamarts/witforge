import { posix } from 'node:path';

export const REPO_BLOB_DOCS_BASE = 'https://github.com/asamarts/witforge/blob/main/docs';

const SCHEME_OR_PROTOCOL_RE = /^(?:[a-z][a-z0-9+.-]*:|\/\/)/i;
const EM_DASH_TEXT_RE = /\u2014|&mdash;|&#0*8212;|&#x0*2014;/gi;
const REAL_HTML_TAG_RE = /<\/?[a-z][a-z0-9-]*(?:\s[^>]*)?>/gi;
const THEMATIC_BREAK_RE = /^(?:(?:-\s*){3,}|(?:\*\s*){3,}|(?:_\s*){3,})$/;
const STANDALONE_IMAGE_RE = /^!\[[^\]]*\]\([^)]+\)$/;

/**
 * Normalize OS path separators to POSIX forward slashes.
 */
export function toPosix(filePath) {
  return String(filePath).replace(/\\/g, '/');
}

/**
 * Normalize a relative path within `docs/` and reject escaping paths.
 */
export function normalizeDocsRelPath(relPath) {
  const cleaned = posix.normalize(toPosix(relPath).replace(/^\.\//, ''));
  if (!cleaned || cleaned === '.' || cleaned.startsWith('../') || cleaned === '..' || cleaned.startsWith('/')) {
    throw new Error(`Invalid docs relative path: ${relPath}`);
  }
  return cleaned;
}

/**
 * @typedef {{ label: string, relPath: string }} SummaryItem
 * @typedef {{ label: string, items: SummaryItem[] }} SummaryGroup
 */

/**
 * Parse `docs/SUMMARY.md` into ordered navigation groups and items.
 *
 * Expected format:
 *   ## Group Label
 *   - [Page Title](relative/path.md)
 *
 * @param {string} markdown
 * @returns {SummaryGroup[]}
 */
export function parseSummary(markdown) {
  const normalized = String(markdown).replace(/\r\n/g, '\n');
  /** @type {SummaryGroup[]} */
  const groups = [];
  const seenPaths = new Set();
  /** @type {SummaryGroup | null} */
  let currentGroup = null;

  for (const [index, rawLine] of normalized.split('\n').entries()) {
    const lineNo = index + 1;
    const trimmed = rawLine.trim();
    if (!trimmed || trimmed.startsWith('# ') || trimmed.startsWith('<!--')) {
      continue;
    }

    const headingMatch = /^##\s+(.+?)\s*$/.exec(trimmed);
    if (headingMatch) {
      currentGroup = { label: headingMatch[1], items: [] };
      groups.push(currentGroup);
      continue;
    }

    const itemMatch = /^-\s+\[([^\]]+)\]\(([^)]+)\)\s*$/.exec(trimmed);
    if (itemMatch) {
      if (!currentGroup) {
        throw new Error(`SUMMARY.md:${lineNo}: list item appears before any "## Section" heading`);
      }
      const label = itemMatch[1].trim();
      const relPath = normalizeDocsRelPath(itemMatch[2].trim());
      if (!/\.md$/i.test(relPath)) {
        throw new Error(`SUMMARY.md:${lineNo}: expected a .md file path, got "${relPath}"`);
      }
      if (seenPaths.has(relPath)) {
        throw new Error(`SUMMARY.md:${lineNo}: duplicate entry "${relPath}"`);
      }
      seenPaths.add(relPath);
      currentGroup.items.push({ label, relPath });
      continue;
    }

    throw new Error(`SUMMARY.md:${lineNo}: unrecognized line "${trimmed}"`);
  }

  if (groups.length === 0) {
    throw new Error('SUMMARY.md must define at least one "## Section" group');
  }
  for (const group of groups) {
    if (group.items.length === 0) {
      throw new Error(`SUMMARY.md section "${group.label}" has no entries`);
    }
  }
  return groups;
}

/**
 * Map a relative path under `docs/` to its Starlight content slug (under `docs/`).
 *
 * - `index.md` -> `docs` (file: `docs/index.md`, URL: `/witforge/docs/`)
 * - `guide/index.md` -> `docs` (canonical alias to `/witforge/docs/`)
 * - `quickstart.md` -> `docs/quickstart`
 * - `guide/<slug>.md` -> `docs/<slug>`
 */
export function docsRelPathToSlug(relPath) {
  const norm = normalizeDocsRelPath(relPath);
  const lower = norm.toLowerCase();
  if (lower === 'index.md' || lower === 'guide/index.md') {
    return 'docs';
  }
  if (lower.startsWith('guide/') && lower.endsWith('.md')) {
    const rest = norm.slice('guide/'.length, -'.md'.length);
    return `docs/${rest}`;
  }
  if (lower.endsWith('.md')) {
    const stem = norm.slice(0, -'.md'.length);
    return `docs/${stem}`;
  }
  return `docs/${norm}`;
}

/**
 * Map a relative path under `docs/` to its target file path inside
 * `site/src/content/docs/`.
 */
export function docsRelPathToContentFile(relPath) {
  const slug = docsRelPathToSlug(relPath);
  if (slug === 'docs') {
    return 'docs/index.md';
  }
  return `${slug}.md`;
}

/**
 * Map a relative path under `docs/` to a canonical site route with `urlPrefix`
 * (e.g. `/witforge` or `https://asamarts.github.io/witforge`).
 */
export function docsRelPathToRoute(relPath, urlPrefix = '/witforge') {
  const prefix = String(urlPrefix).replace(/\/+$/, '');
  const slug = docsRelPathToSlug(relPath);
  return `${prefix}/${slug}/`;
}

/**
 * Strip leading YAML frontmatter (`---\n...\n---`) if present.
 */
export function stripYamlFrontmatter(markdown) {
  const normalized = String(markdown).replace(/\r\n/g, '\n');
  if (!normalized.startsWith('---\n')) {
    return { frontmatter: null, body: normalized };
  }
  const endIndex = normalized.indexOf('\n---\n', 4);
  if (endIndex === -1) {
    if (normalized.endsWith('\n---')) {
      return {
        frontmatter: normalized.slice(4, -4),
        body: '',
      };
    }
    return { frontmatter: null, body: normalized };
  }
  const frontmatter = normalized.slice(4, endIndex);
  const body = normalized.slice(endIndex + '\n---\n'.length).replace(/^\n+/, '');
  return { frontmatter, body };
}

/**
 * Strip a single leading Markdown H1 heading (`# Title`) so Starlight does not
 * duplicate the page heading rendered from frontmatter.
 */
export function stripLeadingH1(body) {
  const lines = String(body).replace(/\r\n/g, '\n').split('\n');
  let i = 0;
  while (i < lines.length && lines[i].trim() === '') {
    i++;
  }
  if (i < lines.length) {
    const match = /^#\s+(.+?)\s*$/.exec(lines[i]);
    if (match) {
      let j = i + 1;
      while (j < lines.length && lines[j].trim() === '') {
        j++;
      }
      return {
        h1: match[1].trim(),
        body: lines.slice(j).join('\n'),
      };
    }
  }
  return { h1: null, body: lines.slice(i).join('\n') };
}

/**
 * Strip inline Markdown syntax while preserving code-span identifiers
 * (such as `snake_case`) and non-HTML angle operators (such as `<->`).
 *
 * @param {string} text
 * @param {{ collapseWhitespace?: boolean }} [options]
 * @returns {string}
 */
export function stripInlineMarkdown(text, { collapseWhitespace = true } = {}) {
  const codeSpans = [];
  let out = '';
  const src = String(text);
  let i = 0;
  const n = src.length;

  while (i < n) {
    if (src[i] === '`') {
      let ticks = 0;
      while (i + ticks < n && src[i + ticks] === '`') ticks++;
      const closer = '`'.repeat(ticks);
      const end = src.indexOf(closer, i + ticks);
      if (end !== -1) {
        const inner = src.slice(i + ticks, end);
        const token = `\uE000${codeSpans.length}\uE001`;
        codeSpans.push(inner);
        out += token;
        i = end + ticks;
        continue;
      }
      out += src.slice(i, i + ticks);
      i += ticks;
      continue;
    }
    out += src[i];
    i++;
  }

  out = out
    .replace(/!\[([^\]]*)\]\([^)]+\)/g, '$1')
    .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
    .replace(/\*\*([^*]+)\*\*/g, '$1')
    .replace(/\*([^*]+)\*/g, '$1')
    .replace(/(^|[^\p{L}\p{N}_])__([^_]+)__(?=[^\p{L}\p{N}_]|$)/gu, '$1$2')
    .replace(/(^|[^\p{L}\p{N}_])_([^_]+)_(?=[^\p{L}\p{N}_]|$)/gu, '$1$2')
    .replace(REAL_HTML_TAG_RE, '');

  out = out.replace(/\uE000(\d+)\uE001/g, (_, idx) => codeSpans[Number(idx)] ?? '');

  if (collapseWhitespace) {
    out = out.replace(/\s+/g, ' ').trim();
  } else {
    out = out.trim();
  }
  return out;
}

/** @type {Record<string, string>} */
export const PAGE_DESCRIPTIONS = {
  'index.md':
    'Overview of witforge, its supported WIT subset, five deterministic emitters, compatibility checks, and guide map.',
  'guide/choosing-witforge.md':
    'When witforge is the right contract compiler, what it leaves out of scope, and when to use wit-bindgen or protobuf directly.',
  'quickstart.md':
    'Five-minute walkthrough from a new WIT contract and wire.lock to generated Rust, Protobuf, JSON Schema, glue, and version diffs.',
  'guide/examples.md':
    'Runnable single-version and evolved contract examples with committed generated outputs checked in CI.',
  'guide/contract-authoring.md':
    'Version directory layout, supported WIT subset (including char, fixed-length lists, and maps), wire.lock, and deps/ packages.',
  'guide/generation.md':
    'Configuring Rust, Protobuf, JSON Schema, glue, and Markdown doc generation with witforge.toml and output ownership headers.',
  'guide/wiring.md':
    'Installing signed prebuilt binaries or crates, wiring a Rust crate with witforge-build, and running an end-to-end round trip.',
  'guide/generated-output.md':
    'Real excerpts of emitted Rust, Protobuf, JSON Schema, checked glue, and version adapters verified against repository examples.',
  'guide/evolution.md':
    'Creating minor and major versions, checking wire continuity, reviewing 4-view diffs, generating convert.rs adapters, and freezing releases.',
  'guide/ci.md':
    'Using the witforge GitHub Action, --check drift gates, release freeze checks, --git-base diffs, policy rules, and SARIF output in CI.',
  'guide/migration.md':
    'Per-release generated-output changes, upgrade notes, and changelog links from 0.1.0 through the current release.',
  'guide/non-rust.md':
    'Generating language bindings from emitted protobuf schemas and implementing the WIT decode profile in non-Rust consumers.',
  'guide/json-projections.md':
    'Differences between Rust Serde JSON, WIT-shaped JSON Schema (Draft 7 and Draft 2020-12), and Proto3 JSON.',
  'guide/component-model.md':
    'Clear responsibilities between witforge, wit-bindgen, and Component Model runtimes like Wasmtime.',
  'guide/reference.md':
    'Complete CLI subcommand, flag, witforge.toml configuration key, and exit code reference verified against the CLI parser.',
  'guide/diagnostics.md':
    'Complete WFxxxx diagnostic code catalog with causes, fixes, source locations, and SARIF rule anchors.',
};

/**
 * Resolve a crisp SEO and llms.txt description for a doc page, preferring the
 * curated map and falling back to sentence-aware paragraph extraction.
 *
 * @param {string} relPath
 * @param {string} body
 * @param {string} label
 * @returns {string}
 */
export function resolvePageDescription(relPath, body, label) {
  const norm = normalizeDocsRelPath(relPath);
  if (Object.hasOwn(PAGE_DESCRIPTIONS, norm)) {
    return PAGE_DESCRIPTIONS[norm];
  }
  return extractDescription(body, `${label} guide for witforge.`);
}

/**
 * Extract a concise plain-text meta description from the first body paragraph.
 */
export function extractDescription(body, fallback = 'witforge documentation.') {
  const lines = String(body).replace(/\r\n/g, '\n').split('\n');
  let inFence = false;
  let fenceChar = '';
  let fenceLen = 0;
  let inComment = false;
  let inList = false;
  const paragraphLines = [];

  for (const line of lines) {
    const trimmed = line.trim();

    if (inComment) {
      if (trimmed.includes('-->')) {
        inComment = false;
      }
      continue;
    }

    if (!inFence && trimmed.startsWith('<!--')) {
      if (!trimmed.includes('-->')) {
        inComment = true;
      }
      continue;
    }

    if (!inFence) {
      const fenceMatch = /^(`{3,}|~{3,})/.exec(trimmed);
      if (fenceMatch) {
        if (paragraphLines.length > 0) break;
        inFence = true;
        fenceChar = fenceMatch[1][0];
        fenceLen = fenceMatch[1].length;
        continue;
      }
    } else {
      const closeRe = new RegExp(`^${fenceChar}{${fenceLen},}\\s*$`);
      if (closeRe.test(trimmed)) {
        inFence = false;
      }
      continue;
    }

    if (!trimmed) {
      inList = false;
      if (paragraphLines.length > 0) {
        const candidate = stripInlineMarkdown(paragraphLines.join(' '));
        if (candidate.endsWith(':') && candidate.length < 110) {
          paragraphLines.length = 0;
          continue;
        }
        break;
      }
      continue;
    }

    const isListStart = /^[-*+]\s+/.test(trimmed) || /^\d+\.\s+/.test(trimmed);
    if (isListStart) {
      if (paragraphLines.length > 0) break;
      inList = true;
      continue;
    }
    if (inList && /^\s+/.test(line)) {
      continue;
    }
    inList = false;

    if (THEMATIC_BREAK_RE.test(trimmed) || STANDALONE_IMAGE_RE.test(trimmed)) {
      if (paragraphLines.length > 0) break;
      continue;
    }

    if (
      paragraphLines.length === 0 &&
      (trimmed.startsWith('#') || trimmed.startsWith('|') || trimmed.startsWith('>'))
    ) {
      continue;
    }

    if (paragraphLines.length > 0 && (trimmed.startsWith('#') || trimmed.startsWith('|'))) {
      break;
    }

    paragraphLines.push(trimmed);
  }

  const plain = stripInlineMarkdown(paragraphLines.join(' '));
  if (!plain) {
    return fallback;
  }
  if (plain.length <= 160) {
    return plain;
  }

  // Prefer the last complete sentence boundary within 160 chars.
  const slice = plain.slice(0, 160);
  const sentenceEnd = slice.lastIndexOf('. ');
  if (sentenceEnd >= 32) {
    return slice.slice(0, sentenceEnd + 1);
  }

  const cut = plain.slice(0, 157);
  const lastSpace = cut.lastIndexOf(' ');
  const clipped = (lastSpace > 80 ? cut.slice(0, lastSpace) : cut)
    .replace(/(?:\s+(?:the|a|an|and|or|of|to|for|in|with|on|by|so))+$/i, '')
    .replace(/[.,;:\s]+$/, '');
  return `${clipped}.`;
}

/**
 * Serialize a single-line string safely for YAML frontmatter.
 */
export function yamlString(value) {
  return JSON.stringify(String(value).replace(/\r?\n/g, ' ').trim());
}

/**
 * Generate a GitHub/Starlight (`github-slugger`) compatible heading anchor ID.
 * Pass an optional `counts` Map to deduplicate repeated headings on one page.
 */
export function githubSlug(heading, counts = null) {
  const plain = stripInlineMarkdown(heading, { collapseWhitespace: false }).toLowerCase();
  const base = plain
    .replace(/[^\p{L}\p{N}\s_-]/gu, '')
    .replace(/\s/g, '-');

  if (!counts) {
    return base;
  }
  const seen = counts.get(base) ?? 0;
  counts.set(base, seen + 1);
  return seen === 0 ? base : `${base}-${seen}`;
}

function isExcludedDocsPath(normRelPath) {
  const lower = normRelPath.toLowerCase();
  return (
    lower === 'aplan-pilot.md' ||
    lower === 'summary.md' ||
    lower === 'design' ||
    lower.startsWith('design/')
  );
}

/**
 * Rewrite a single Markdown link target relative to `relPath` within `docs/`.
 */
export function rewriteLinkTarget(rawTarget, relPath, urlPrefix = '/witforge') {
  const trimmed = String(rawTarget).trim();
  if (
    !trimmed ||
    trimmed.startsWith('#') ||
    trimmed.startsWith('?') ||
    trimmed.startsWith('/') ||
    SCHEME_OR_PROTOCOL_RE.test(trimmed)
  ) {
    return rawTarget;
  }

  const suffixIdx = trimmed.search(/[?#]/);
  const pathPart = suffixIdx === -1 ? trimmed : trimmed.slice(0, suffixIdx);
  const suffix = suffixIdx === -1 ? '' : trimmed.slice(suffixIdx);

  if (!pathPart) {
    return rawTarget;
  }

  const relDir = posix.dirname(normalizeDocsRelPath(relPath));
  const normalizedInput = pathPart === '.' ? './' : pathPart;
  let resolved = posix.normalize(
    relDir === '.' ? normalizedInput : posix.join(relDir, normalizedInput),
  );

  if (normalizedInput.endsWith('/') && !resolved.endsWith('/')) {
    resolved = `${resolved}/`;
  }

  if (resolved === '.' || resolved === './') {
    resolved = relDir === '.' ? 'index.md' : posix.join(relDir, 'index.md');
  } else if (resolved.endsWith('/')) {
    resolved = posix.join(resolved, 'index.md');
  }

  if (resolved.startsWith('../') || resolved === '..' || resolved.startsWith('/')) {
    return rawTarget;
  }

  if (!/\.md$/i.test(resolved)) {
    return rawTarget;
  }

  if (isExcludedDocsPath(resolved)) {
    return `${REPO_BLOB_DOCS_BASE}/${resolved}${suffix}`;
  }

  return `${docsRelPathToRoute(resolved, urlPrefix)}${suffix}`;
}

/**
 * Rewrite inline Markdown links `[label](target)` on a single prose line while
 * skipping inline backtick code spans and backslash-escaped delimiters.
 */
function rewriteProseLine(line, relPath, urlPrefix) {
  // Handle Markdown reference link definitions: `[id]: target "optional title"`
  const refDefMatch = /^(\s*\[[^\]]+\]:\s*)(<[^>]+>|\S+)(\s*(?:["'(].*)?)$/.exec(line);
  if (refDefMatch) {
    const [, prefix, rawUrl, suffix] = refDefMatch;
    const unwrapped = rawUrl.startsWith('<') && rawUrl.endsWith('>') ? rawUrl.slice(1, -1) : rawUrl;
    const rewritten = rewriteLinkTarget(unwrapped, relPath, urlPrefix);
    const wrapped = rawUrl.startsWith('<') && rawUrl.endsWith('>') ? `<${rewritten}>` : rewritten;
    return `${prefix}${wrapped}${suffix}`;
  }

  let out = '';
  let i = 0;
  const n = line.length;

  while (i < n) {
    // Preserve backslash-escaped characters (e.g. `\[not a link](foo.md)`).
    if (line[i] === '\\' && i + 1 < n) {
      out += line.slice(i, i + 2);
      i += 2;
      continue;
    }

    // Skip inline code spans (matching exact backtick count).
    if (line[i] === '`') {
      let ticks = 0;
      while (i + ticks < n && line[i + ticks] === '`') {
        ticks++;
      }
      const closer = '`'.repeat(ticks);
      const end = line.indexOf(closer, i + ticks);
      if (end !== -1) {
        out += line.slice(i, end + ticks);
        i = end + ticks;
        continue;
      }
      out += line.slice(i, i + ticks);
      i += ticks;
      continue;
    }

    // Match `[label](target)` (skipping image syntax `![alt](src)`).
    if (line[i] === '[' && (i === 0 || line[i - 1] !== '!')) {
      const closeBracket = findMatchingBracket(line, i);
      if (closeBracket !== -1 && line[closeBracket + 1] === '(') {
        const closeParen = findMatchingParen(line, closeBracket + 1);
        if (closeParen !== -1) {
          const label = line.slice(i + 1, closeBracket);
          const inner = line.slice(closeBracket + 2, closeParen);
          const targetMatch = /^(\s*)(<[^>]+>|[^\s]+)(\s+[\s\S]*)?$/.exec(inner);
          if (targetMatch) {
            const [, leadWs, rawUrl, tail = ''] = targetMatch;
            const unwrapped =
              rawUrl.startsWith('<') && rawUrl.endsWith('>') ? rawUrl.slice(1, -1) : rawUrl;
            const rewritten = rewriteLinkTarget(unwrapped, relPath, urlPrefix);
            const wrapped =
              rawUrl.startsWith('<') && rawUrl.endsWith('>') ? `<${rewritten}>` : rewritten;
            out += `[${label}](${leadWs}${wrapped}${tail})`;
            i = closeParen + 1;
            continue;
          }
        }
      }
    }

    out += line[i];
    i++;
  }

  return out;
}

function findMatchingBracket(line, openIdx) {
  let depth = 0;
  for (let i = openIdx; i < line.length; i++) {
    if (line[i] === '\\') {
      i++;
      continue;
    }
    if (line[i] === '`') {
      let ticks = 0;
      while (i + ticks < line.length && line[i + ticks] === '`') ticks++;
      const end = line.indexOf('`'.repeat(ticks), i + ticks);
      if (end !== -1) {
        i = end + ticks - 1;
        continue;
      }
    }
    if (line[i] === '[') depth++;
    else if (line[i] === ']') {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

function findMatchingParen(line, openIdx) {
  let depth = 0;
  for (let i = openIdx; i < line.length; i++) {
    if (line[i] === '\\') {
      i++;
      continue;
    }
    if (line[i] === '(') depth++;
    else if (line[i] === ')') {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/**
 * Rewrite relative `.md` links across a Markdown body while skipping fenced
 * code blocks and inline code spans.
 */
export function rewriteMarkdownLinks(body, relPath, urlPrefix = '/witforge') {
  const lines = String(body).replace(/\r\n/g, '\n').split('\n');
  const out = [];
  let inFence = false;
  let fenceChar = '';
  let fenceLen = 0;

  for (const line of lines) {
    const leading = line.trimStart();
    if (!inFence) {
      const m = /^(`{3,}|~{3,})/.exec(leading);
      if (m) {
        inFence = true;
        fenceChar = m[1][0];
        fenceLen = m[1].length;
        out.push(line);
        continue;
      }
      out.push(rewriteProseLine(line, relPath, urlPrefix));
    } else {
      const closeRe = new RegExp(`^${fenceChar}{${fenceLen},}\\s*$`);
      if (closeRe.test(leading)) {
        inFence = false;
      }
      out.push(line);
    }
  }

  return out.join('\n');
}

/**
 * Find em-dash occurrences (`U+2014`, `&mdash;`, `&#0*8212;`, `&#x0*2014;`) in plain text.
 * Returns array of `{ line, match, excerpt }`.
 */
export function findEmDashesInText(text) {
  const findings = [];
  const lines = String(text).replace(/\r\n/g, '\n').split('\n');
  for (let idx = 0; idx < lines.length; idx++) {
    const line = lines[idx];
    EM_DASH_TEXT_RE.lastIndex = 0;
    let m;
    while ((m = EM_DASH_TEXT_RE.exec(line)) !== null) {
      findings.push({
        line: idx + 1,
        match: m[0],
        excerpt: line.trim().slice(0, 120),
      });
    }
  }
  return findings;
}

function stripPairedTagBlocks(src, tagName) {
  const re = new RegExp(`<${tagName}\\b[^>]*>(?:(?!<${tagName}\\b)[\\s\\S])*?<\\/${tagName}\\s*>`, 'gi');
  let prev = src;
  while (true) {
    const next = prev.replace(re, (block) => block.replace(/[^\n]/g, ' '));
    if (next === prev) return next;
    prev = next;
  }
}

/**
 * Find em-dash occurrences in rendered HTML prose (excluding `<pre>`, `<code>`,
 * `<script>`, and `<style>` blocks, while scanning visible text, `<title>`,
 * meta description/title tags including OpenGraph `property="..."`, and SVG
 * `<text>`/`<title>`/`<desc>`).
 */
export function findEmDashesInHtml(html) {
  const findings = [];
  const src = String(html).replace(/\r\n/g, '\n');

  // Inspect meta description and title tags (`name="..."` and OpenGraph `property="..."`).
  const metaRe =
    /<meta\b[^>]*\b(?:name|property)=["'](?:description|og:description|og:title|twitter:description|twitter:title)["'][^>]*>/gi;
  let metaMatch;
  while ((metaMatch = metaRe.exec(src)) !== null) {
    const contentMatch = /\bcontent=(["'])([\s\S]*?)\1/i.exec(metaMatch[0]);
    if (contentMatch) {
      const metaLine = src.slice(0, metaMatch.index).split('\n').length;
      for (const hit of findEmDashesInText(contentMatch[2])) {
        findings.push({
          line: metaLine,
          match: hit.match,
          excerpt: `meta tag: ${hit.excerpt}`,
        });
      }
    }
  }

  // Strip script, style, code (innermost first for nested code), pre, comments, and tags.
  let stripped = src;
  stripped = stripPairedTagBlocks(stripped, 'script');
  stripped = stripPairedTagBlocks(stripped, 'style');
  stripped = stripPairedTagBlocks(stripped, 'code');
  stripped = stripPairedTagBlocks(stripped, 'pre');
  stripped = stripped
    .replace(/<!--[\s\S]*?-->/g, (comment) => comment.replace(/[^\n]/g, ' '))
    .replace(/<[^>]+>/g, ' ');

  findings.push(...findEmDashesInText(stripped));
  return findings;
}
