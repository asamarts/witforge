import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  docsRelPathToContentFile,
  docsRelPathToRoute,
  docsRelPathToSlug,
  extractDescription,
  findEmDashesInHtml,
  findEmDashesInText,
  githubSlug,
  normalizeDocsRelPath,
  parseSummary,
  rewriteLinkTarget,
  rewriteMarkdownLinks,
  stripLeadingH1,
  stripYamlFrontmatter,
  toPosix,
  yamlString,
} from './markdown-utils.mjs';
import { syncDocs } from './sync-docs.mjs';

const SCRIPTS_DIR = dirname(fileURLToPath(import.meta.url));
const SITE_ROOT = resolve(SCRIPTS_DIR, '..');
const REPO_ROOT = resolve(SITE_ROOT, '..');
const DOCS_ROOT = resolve(REPO_ROOT, 'docs');

function makeTempDir(prefix) {
  const base = process.env.TMPDIR || tmpdir();
  mkdirSync(base, { recursive: true });
  return mkdtempSync(join(base, prefix));
}

test('toPosix and normalizeDocsRelPath handle Windows separators and reject escapes', () => {
  assert.equal(toPosix('guide\\wiring.md'), 'guide/wiring.md');
  assert.equal(normalizeDocsRelPath('./guide\\wiring.md'), 'guide/wiring.md');
  assert.throws(() => normalizeDocsRelPath('../README.md'), /Invalid docs relative path/);
  assert.throws(() => normalizeDocsRelPath('/etc/passwd'), /Invalid docs relative path/);
});

test('parseSummary parses real docs/SUMMARY.md into 5 groups and 16 pages', () => {
  const raw = readFileSync(join(DOCS_ROOT, 'SUMMARY.md'), 'utf8');
  const groups = parseSummary(raw);
  assert.equal(groups.length, 5);
  const totalItems = groups.reduce((sum, g) => sum + g.items.length, 0);
  assert.equal(totalItems, 16);
  assert.equal(groups[0].label, 'Getting Started');
  assert.equal(groups[0].items[0].relPath, 'index.md');
});

test('parseSummary rejects duplicates, escaping paths, and empty sections', () => {
  assert.throws(
    () => parseSummary('## One\n- [A](index.md)\n- [B](index.md)\n'),
    /duplicate entry/,
  );
  assert.throws(() => parseSummary('## One\n- [A](../README.md)\n'), /Invalid docs relative path/);
  assert.throws(() => parseSummary('## Empty\n'), /has no entries/);
  assert.throws(() => parseSummary('- [Orphan](index.md)\n'), /before any/);
});

test('slug, content file, and route mapping preserve clean flat URLs', () => {
  assert.equal(docsRelPathToSlug('index.md'), 'docs');
  assert.equal(docsRelPathToSlug('guide/index.md'), 'docs');
  assert.equal(docsRelPathToSlug('quickstart.md'), 'docs/quickstart');
  assert.equal(docsRelPathToSlug('guide/diagnostics.md'), 'docs/diagnostics');

  assert.equal(docsRelPathToContentFile('index.md'), 'docs/index.md');
  assert.equal(docsRelPathToContentFile('quickstart.md'), 'docs/quickstart.md');
  assert.equal(docsRelPathToContentFile('guide/wiring.md'), 'docs/wiring.md');

  assert.equal(docsRelPathToRoute('index.md'), '/witforge/docs/');
  assert.equal(docsRelPathToRoute('guide/wiring.md'), '/witforge/docs/wiring/');
  assert.equal(
    docsRelPathToRoute('guide/diagnostics.md', 'https://asamarts.github.io/witforge'),
    'https://asamarts.github.io/witforge/docs/diagnostics/',
  );
});

test('stripYamlFrontmatter and stripLeadingH1 preserve body and horizontal rules', () => {
  const doc = '---\r\nlayout: default\r\ntitle: Test\r\n---\r\n\r\n# Heading\r\n\r\nPara.\r\n\r\n---\r\n\r\nAfter.\r\n';
  const { frontmatter, body } = stripYamlFrontmatter(doc);
  assert.ok(frontmatter?.includes('layout: default'));
  const { h1, body: clean } = stripLeadingH1(body);
  assert.equal(h1, 'Heading');
  assert.equal(clean, 'Para.\n\n---\n\nAfter.\n');
});

test('extractDescription strips Markdown formatting, prefers complete sentences, and skips list intros', () => {
  const md = [
    '## Section',
    '',
    'Short intro:',
    '',
    '- bullet item',
    '  wrapped continuation line',
    '',
    '```wit',
    'package a:b@0.1.0;',
    '```',
    '',
    'Use [`witforge lock`](guide/reference.md) for **stable** wire numbers. This second sentence pushes the total paragraph length well past one hundred and sixty characters so the trimmer should stop cleanly at the first sentence.',
  ].join('\n');
  assert.equal(
    extractDescription(md),
    'Use witforge lock for stable wire numbers.',
  );
  assert.equal(yamlString('Title: with "quotes"'), '"Title: with \\"quotes\\""');
});

test('githubSlug matches lowercase wfxxxx anchors for every documented diagnostic code', () => {
  const diagnosticsMd = readFileSync(join(DOCS_ROOT, 'guide', 'diagnostics.md'), 'utf8');
  const codes = [...diagnosticsMd.matchAll(/^###\s+(WF\d{4})\s*$/gm)].map((m) => m[1]);
  assert.ok(codes.length >= 50, `Expected >= 50 diagnostic headings, found ${codes.length}`);
  const counts = new Map();
  for (const code of codes) {
    assert.equal(githubSlug(code, counts), code.toLowerCase());
  }
});

test('rewriteMarkdownLinks rewrites doc links while preserving fragments and code blocks', () => {
  assert.equal(
    rewriteLinkTarget('../quickstart.md#step-2', 'guide/wiring.md'),
    '/witforge/docs/quickstart/#step-2',
  );
  assert.equal(
    rewriteLinkTarget('guide/wiring.md#prebuilt-binaries', 'index.md'),
    '/witforge/docs/wiring/#prebuilt-binaries',
  );
  assert.equal(rewriteLinkTarget('index.md', 'guide/wiring.md'), '/witforge/docs/');
  assert.equal(rewriteLinkTarget('./', 'guide/wiring.md'), '/witforge/docs/');
  assert.equal(
    rewriteLinkTarget('design/model.md#shape', 'index.md'),
    'https://github.com/asamarts/witforge/blob/main/docs/design/model.md#shape',
  );
  assert.equal(
    rewriteLinkTarget('https://docs.rs/witforge', 'index.md'),
    'https://docs.rs/witforge',
  );

  const md = [
    'See [wiring](guide/wiring.md#install) and `[literal](guide/wiring.md)` and `` `[nested](guide/ci.md)` ``.',
    '',
    '```markdown',
    '[inside fence](guide/wiring.md)',
    '```',
    '',
    '[ref]: guide/evolution.md#adapters "Adapters"',
  ].join('\n');

  const rewritten = rewriteMarkdownLinks(md, 'index.md', '/witforge');
  assert.ok(rewritten.includes('See [wiring](/witforge/docs/wiring/#install)'));
  assert.ok(rewritten.includes('`[literal](guide/wiring.md)`'));
  assert.ok(rewritten.includes('`` `[nested](guide/ci.md)` ``'));
  assert.ok(rewritten.includes('[inside fence](guide/wiring.md)'));
  assert.ok(rewritten.includes('[ref]: /witforge/docs/evolution/#adapters "Adapters"'));
});

test('findEmDashesInText and findEmDashesInHtml catch prose dashes and ignore code blocks', () => {
  assert.equal(findEmDashesInText('Clean sentence, no dashes.').length, 0);
  assert.equal(findEmDashesInText('Bad \u2014 dash and &mdash; and &#8212; and &#x2014;.').length, 4);

  const cleanHtml =
    '<html><head><title>witforge</title><meta name="description" content="Clean desc"></head>' +
    '<body><pre><code>// \u2014 inside code is ignored</code></pre><p>Clean prose.</p></body></html>';
  assert.equal(findEmDashesInHtml(cleanHtml).length, 0);

  const dirtyHtml =
    '<html><head><meta name="description" content="Desc \u2014 bad"></head>' +
    '<body><p>Prose &mdash; bad</p></body></html>';
  assert.equal(findEmDashesInHtml(dirtyHtml).length, 2);
});

test('syncDocs writes deterministic content and sidebar into a temporary site root', () => {
  const scratchSite = makeTempDir('witforge-sync-test-');
  try {
    const result = syncDocs({ docsRoot: DOCS_ROOT, siteRoot: scratchSite });
    assert.equal(result.pageCount, 16);
    assert.equal(result.groupCount, 5);

    const sidebarPath = join(scratchSite, 'src', 'data', 'sidebar.json');
    assert.ok(existsSync(sidebarPath));
    const sidebar = JSON.parse(readFileSync(sidebarPath, 'utf8'));
    assert.equal(sidebar.length, 5);
    assert.equal(sidebar[0].items[0].slug, 'docs');

    const overviewPath = join(scratchSite, 'src', 'content', 'docs', 'docs', 'index.md');
    const overview = readFileSync(overviewPath, 'utf8');
    assert.ok(overview.startsWith('---\ntitle: "Overview"\n'));
    assert.ok(overview.includes('editUrl: "https://github.com/asamarts/witforge/edit/main/docs/index.md"'));
    assert.ok(overview.includes('(/witforge/docs/quickstart/)'));
    assert.ok(!overview.includes('layout: default'));
  } finally {
    rmSync(scratchSite, { recursive: true, force: true });
  }
});
