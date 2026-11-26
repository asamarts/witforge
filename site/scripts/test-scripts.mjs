import assert from 'node:assert/strict';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  checkSite,
  findProseIssuesInHtml,
  findProseIssuesInPlainText,
  validateSvgDiagramsInHtml,
} from './check-links.mjs';
import {
  emitRedirects,
  escapeHtml,
  escapeJsString,
  loadRedirects,
  REDIRECT_HEADER,
  REDIRECT_MARKER,
  renderRedirectHtml,
} from './emit-redirects.mjs';
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
  stripInlineMarkdown,
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
  assert.equal(docsRelPathToSlug('guide/Wiring.MD'), 'docs/Wiring');

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

test('stripInlineMarkdown preserves snake_case code and non-HTML angle operators', () => {
  assert.equal(stripInlineMarkdown('Run `cargo_test_foo` and __bold__ and _italic_.'), 'Run cargo_test_foo and bold and italic.');
  assert.equal(stripInlineMarkdown('foo_bar_baz stays intact'), 'foo_bar_baz stays intact');
  assert.equal(stripInlineMarkdown('WIT <-> Rust <span>tag</span>'), 'WIT <-> Rust tag');
});

test('extractDescription strips Markdown, prefers sentences, and skips comments/rules/images/lists', () => {
  const md = [
    '<!--',
    'multiline comment interior',
    '-->',
    '',
    '---',
    '',
    '![diagram](diagram.svg)',
    '',
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
    'Use [`witforge lock`](guide/reference.md) for **stable** `wire_lock` numbers. This second sentence pushes the total paragraph length well past one hundred and sixty characters so the trimmer should stop cleanly at the first sentence.',
  ].join('\n');
  assert.equal(
    extractDescription(md),
    'Use witforge lock for stable wire_lock numbers.',
  );
  assert.equal(yamlString('Title: with "quotes"'), '"Title: with \\"quotes\\""');
});

test('githubSlug matches github-slugger on snake_case, operators, and all documented diagnostic codes', () => {
  assert.equal(githubSlug('foo_bar_baz'), 'foo_bar_baz');
  assert.equal(githubSlug('`v0_1` -> `v0_2`'), 'v0_1---v0_2');
  assert.equal(githubSlug('WIT <-> Rust'), 'wit---rust');

  const diagnosticsMd = readFileSync(join(DOCS_ROOT, 'guide', 'diagnostics.md'), 'utf8');
  const codes = [...diagnosticsMd.matchAll(/^###\s+(WF\d{4})\s*$/gm)].map((m) => m[1]);
  assert.ok(codes.length >= 50, `Expected >= 50 diagnostic headings, found ${codes.length}`);
  const counts = new Map();
  for (const code of codes) {
    assert.equal(githubSlug(code, counts), code.toLowerCase());
  }
});

test('rewriteMarkdownLinks handles root-relative paths, queries, dot dirs, uppercase .MD, and escaped brackets', () => {
  // Root-relative links must pass through untouched from both top-level and nested docs
  assert.equal(rewriteLinkTarget('/witforge/docs/', 'index.md'), '/witforge/docs/');
  assert.equal(rewriteLinkTarget('/witforge/docs/', 'guide/wiring.md'), '/witforge/docs/');
  assert.equal(rewriteLinkTarget('/outputs/', 'guide/wiring.md'), '/outputs/');

  // Relative paths, fragments, queries, bare dot, and case-insensitive .MD
  assert.equal(
    rewriteLinkTarget('../quickstart.md#step-2', 'guide/wiring.md'),
    '/witforge/docs/quickstart/#step-2',
  );
  assert.equal(
    rewriteLinkTarget('guide/wiring.md?tab=cargo#prebuilt-binaries', 'index.md'),
    '/witforge/docs/wiring/?tab=cargo#prebuilt-binaries',
  );
  assert.equal(rewriteLinkTarget('index.md', 'guide/wiring.md'), '/witforge/docs/');
  assert.equal(rewriteLinkTarget('./', 'guide/wiring.md'), '/witforge/docs/');
  assert.equal(rewriteLinkTarget('.', 'guide/wiring.md'), '/witforge/docs/');
  assert.equal(rewriteLinkTarget('guide/wiring.MD', 'index.md'), '/witforge/docs/wiring/');
  assert.equal(
    rewriteLinkTarget('design/model.md#shape', 'index.md'),
    'https://github.com/asamarts/witforge/blob/main/docs/design/model.md#shape',
  );
  assert.equal(
    rewriteLinkTarget('https://docs.rs/witforge', 'index.md'),
    'https://docs.rs/witforge',
  );

  const md = [
    'See [wiring](guide/wiring.md#install), \\[escaped](guide/wiring.md), `[literal](guide/wiring.md)` and `` `[nested](guide/ci.md)` ``.',
    '',
    '```markdown',
    '[inside fence](guide/wiring.md)',
    '```',
    '',
    '[ref]: guide/evolution.md#adapters "Adapters"',
  ].join('\n');

  const rewritten = rewriteMarkdownLinks(md, 'index.md', '/witforge');
  assert.ok(rewritten.includes('See [wiring](/witforge/docs/wiring/#install)'));
  assert.ok(rewritten.includes('\\[escaped](guide/wiring.md)'));
  assert.ok(rewritten.includes('`[literal](guide/wiring.md)`'));
  assert.ok(rewritten.includes('`` `[nested](guide/ci.md)` ``'));
  assert.ok(rewritten.includes('[inside fence](guide/wiring.md)'));
  assert.ok(rewritten.includes('[ref]: /witforge/docs/evolution/#adapters "Adapters"'));
});

test('findEmDashesInText and findEmDashesInHtml catch OG meta tags, padded entities, and ignore nested code', () => {
  assert.equal(findEmDashesInText('Clean sentence, no dashes.').length, 0);
  assert.equal(
    findEmDashesInText('Bad \u2014 dash, &mdash;, &#8212;, &#08212;, &#x2014;, &#x02014;.').length,
    6,
  );

  const cleanHtml =
    '<html><head><title>witforge</title><meta property="og:description" content="Clean desc"></head>' +
    '<body><pre><code>outer <code>\u2014 inner</code> end</code ></pre><p>Clean prose.</p></body></html>';
  assert.equal(findEmDashesInHtml(cleanHtml).length, 0);

  const dirtyHtml = [
    '<html><head>',
    '<meta name="description" content="Desc \u2014 bad">',
    '<meta property="og:description" content="OG \u2014 bad">',
    '<meta property="og:title" content="Title &mdash; bad">',
    '</head><body><p>Prose &#08212; bad</p></body></html>',
  ].join('\n');
  const hits = findEmDashesInHtml(dirtyHtml);
  assert.equal(hits.length, 4);
  assert.equal(hits[1].line, 3);
  assert.equal(hits[2].line, 4);
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

test('redirects.json covers /guide/index.html, /quickstart.html, and every docs/guide/*.md page', () => {
  const entries = loadRedirects();
  assert.equal(entries.length, 16);

  const fromSet = new Set(entries.map((entry) => entry.from));
  assert.ok(fromSet.has('/guide/index.html'));
  assert.ok(fromSet.has('/quickstart.html'));

  const guidePages = readdirSync(join(DOCS_ROOT, 'guide'))
    .filter((name) => name.endsWith('.md') && name !== 'index.md')
    .sort();
  assert.equal(guidePages.length, 14);
  for (const filename of guidePages) {
    const slug = filename.replace(/\.md$/, '');
    const entry = entries.find((item) => item.from === `/guide/${slug}.html`);
    assert.ok(entry, `Missing redirect entry for /guide/${slug}.html`);
    assert.equal(entry.to, `/witforge/docs/${slug}/`);
  }
});

test('renderRedirectHtml emits canonical URL, noindex, fragment-preserving JS, noscript refresh, and escapes HTML/JS', () => {
  const html = renderRedirectHtml({
    to: '/witforge/docs/diagnostics/',
    title: 'Diagnostic codes & "quotes" <script>',
  });

  assert.ok(html.startsWith(`<!DOCTYPE html>\n${REDIRECT_MARKER}\n`));
  assert.ok(html.includes('<meta name="robots" content="noindex">'));
  assert.ok(
    html.includes(
      '<link rel="canonical" href="https://asamarts.github.io/witforge/docs/diagnostics/">',
    ),
  );
  assert.ok(
    html.includes(
      '<noscript><meta http-equiv="refresh" content="0; url=/witforge/docs/diagnostics/"></noscript>',
    ),
  );
  assert.ok(
    html.includes('window.location.replace(target + window.location.search + window.location.hash);'),
  );
  assert.ok(
    html.includes(
      '<a href="/witforge/docs/diagnostics/">Diagnostic codes &amp; &quot;quotes&quot; &lt;script&gt;</a>',
    ),
  );
  assert.equal(findEmDashesInHtml(html).length, 0);

  assert.equal(escapeHtml(`a & <b> "c" 'd'`), 'a &amp; &lt;b&gt; &quot;c&quot; &#39;d&#39;');
  assert.equal(escapeJsString('</script>&'), '"\\u003c/script\\u003e\\u0026"');

  // Blank or whitespace-only title falls back to the target URL rather than empty anchor text
  const blankTitle = renderRedirectHtml({ to: '/witforge/docs/', title: '   ' });
  assert.ok(blankTitle.includes('<a href="/witforge/docs/">/witforge/docs/</a>'));
  assert.throws(
    () => renderRedirectHtml({ to: '/witforge/docs/', siteOrigin: 'javascript:alert(1)' }),
    /Invalid siteOrigin/,
  );
  assert.throws(
    () => renderRedirectHtml({ to: '/witforge/docs/', siteOrigin: 'https://example.com/sub' }),
    /must not include a path/,
  );
});

test('emitRedirects writes all 16 stubs, is idempotent, and refuses missing targets or route collisions', () => {
  const scratchDist = makeTempDir('witforge-redirects-test-');
  try {
    const entries = loadRedirects();
    for (const entry of entries) {
      const sub = entry.to.replace(/^\/witforge\//, '').replace(/\/$/, '');
      const destFile = sub
        ? join(scratchDist, ...sub.split('/'), 'index.html')
        : join(scratchDist, 'index.html');
      mkdirSync(dirname(destFile), { recursive: true });
      writeFileSync(destFile, '<!DOCTYPE html><html><body>Canonical page</body></html>\n', 'utf8');
    }

    const firstRun = emitRedirects({ distDir: scratchDist });
    assert.equal(firstRun.count, 16);
    assert.ok(firstRun.written.includes('guide/index.html'));
    assert.ok(firstRun.written.includes('guide/diagnostics.html'));
    assert.ok(firstRun.written.includes('quickstart.html'));

    const diagStub = readFileSync(join(scratchDist, 'guide', 'diagnostics.html'), 'utf8');
    assert.ok(diagStub.includes('https://asamarts.github.io/witforge/docs/diagnostics/'));
    assert.ok(diagStub.includes('window.location.search + window.location.hash'));
    assert.equal(findEmDashesInHtml(diagStub).length, 0);

    // Re-running on the same dist directory is idempotent because stubs start with REDIRECT_HEADER
    const secondRun = emitRedirects({ distDir: scratchDist });
    assert.equal(secondRun.count, 16);

    // Collision with a canonical route that merely mentions REDIRECT_MARKER in body prose must fail
    const collisionPath = join(scratchDist, 'quickstart.html');
    writeFileSync(
      collisionPath,
      `<!DOCTYPE html><html><body><p>Example marker: ${REDIRECT_MARKER}</p></body></html>\n`,
      'utf8',
    );
    assert.throws(
      () => emitRedirects({ distDir: scratchDist }),
      /collides with an existing canonical route/,
    );

    // Restore valid stub header, then remove a destination page and ensure missing destination fails atomically
    writeFileSync(collisionPath, `${REDIRECT_HEADER}`, 'utf8');
    rmSync(join(scratchDist, 'docs', 'examples', 'index.html'));
    rmSync(join(scratchDist, 'guide', 'ci.html'));
    assert.throws(
      () => emitRedirects({ distDir: scratchDist }),
      /Redirect target does not exist in dist/,
    );
    assert.ok(
      !existsSync(join(scratchDist, 'guide', 'ci.html')),
      'Must not write partial stubs on validation failure',
    );
  } finally {
    rmSync(scratchDist, { recursive: true, force: true });
  }
});

test('emitRedirects refuses symlinks, ancestor file collisions, self-redirects, chains, and bad distDir', () => {
  const scratchRoot = makeTempDir('witforge-redirects-hardening-');
  try {
    assert.throws(
      () => emitRedirects({ distDir: join(scratchRoot, 'missing') }),
      /Built dist directory not found/,
    );

    const fileAsDist = join(scratchRoot, 'not-a-dir');
    writeFileSync(fileAsDist, 'file', 'utf8');
    assert.throws(() => emitRedirects({ distDir: fileAsDist }), /not a directory/);

    const dist = join(scratchRoot, 'dist');
    const manifest = join(scratchRoot, 'manifest.json');
    mkdirSync(join(dist, 'docs', 'target'), { recursive: true });
    writeFileSync(join(dist, 'docs', 'target', 'index.html'), '<!DOCTYPE html><html></html>\n', 'utf8');
    writeFileSync(
      manifest,
      JSON.stringify([
        { from: '/first.html', to: '/witforge/docs/target/', title: 'Target' },
        { from: '/guide/nested.html', to: '/witforge/docs/target/', title: 'Target' },
      ]),
      'utf8',
    );

    // File blocking ancestor directory ("dist/guide" is a file) must fail atomically before writing /first.html
    writeFileSync(join(dist, 'guide'), 'blocking file', 'utf8');
    assert.throws(
      () => emitRedirects({ distDir: dist, manifestPath: manifest }),
      /collides with non-directory path/,
    );
    assert.ok(!existsSync(join(dist, 'first.html')), 'Must not write earlier stubs when ancestor fails');
    rmSync(join(dist, 'guide'));

    // Directory at outFile ("dist/first.html/") must fail
    mkdirSync(join(dist, 'first.html'));
    assert.throws(
      () => emitRedirects({ distDir: dist, manifestPath: manifest }),
      /collides with non-file path/,
    );
    rmSync(join(dist, 'first.html'), { recursive: true });

    // Dangling symlink at outFile and symlinked ancestor directory must both be refused
    if (process.platform !== 'win32') {
      const outside = join(scratchRoot, 'outside.html');
      symlinkSync(outside, join(dist, 'first.html'));
      assert.throws(
        () => emitRedirects({ distDir: dist, manifestPath: manifest }),
        /Refusing symlinked redirect output path/,
      );
      assert.ok(!existsSync(outside), 'Must never follow dangling symlink outside dist');
      rmSync(join(dist, 'first.html'));

      const outsideDir = join(scratchRoot, 'outside-dir');
      mkdirSync(outsideDir);
      symlinkSync(outsideDir, join(dist, 'guide'));
      assert.throws(
        () => emitRedirects({ distDir: dist, manifestPath: manifest }),
        /Refusing symlinked directory in dist/,
      );
      rmSync(join(dist, 'guide'));
    }

    // Self-redirect loop and redirect-to-redirect chain must be refused even on re-runs
    const loopManifest = join(scratchRoot, 'loop.json');
    writeFileSync(
      loopManifest,
      JSON.stringify([{ from: '/guide/index.html', to: '/witforge/guide/' }]),
      'utf8',
    );
    assert.throws(
      () => emitRedirects({ distDir: dist, manifestPath: loopManifest }),
      /loop or chain/,
    );

    // Pre-existing redirect stub at target must also be refused as a chain
    mkdirSync(join(dist, 'docs', 'stub-target'), { recursive: true });
    writeFileSync(join(dist, 'docs', 'stub-target', 'index.html'), `${REDIRECT_HEADER}`, 'utf8');
    const chainManifest = join(scratchRoot, 'chain.json');
    writeFileSync(
      chainManifest,
      JSON.stringify([{ from: '/chain.html', to: '/witforge/docs/stub-target/' }]),
      'utf8',
    );
    assert.throws(
      () => emitRedirects({ distDir: dist, manifestPath: chainManifest }),
      /itself a redirect stub/,
    );

    // Root basePath "/" works cleanly
    const rootManifest = join(scratchRoot, 'root-base.json');
    writeFileSync(
      rootManifest,
      JSON.stringify([{ from: '/old.html', to: '/docs/target/', title: 'Target' }]),
      'utf8',
    );
    const rootResult = emitRedirects({ distDir: dist, manifestPath: rootManifest, basePath: '/' });
    assert.equal(rootResult.count, 1);
    const rootHtml = readFileSync(join(dist, 'old.html'), 'utf8');
    assert.ok(rootHtml.includes('<link rel="canonical" href="https://asamarts.github.io/docs/target/">'));
  } finally {
    rmSync(scratchRoot, { recursive: true, force: true });
  }
});

test('loadRedirects rejects invalid, duplicate, reserved, prefix-colliding, malformed, or escaping entries', () => {
  const scratchDir = makeTempDir('witforge-redirects-manifest-');
  try {
    const badJsonFile = join(scratchDir, 'bad-json.json');
    writeFileSync(badJsonFile, '{ not valid json', 'utf8');
    assert.throws(() => loadRedirects(badJsonFile), /Invalid JSON in .*bad-json\.json/);

    const badCases = [
      [],
      { from: '/guide/a.html', to: '/witforge/docs/' },
      [null],
      [{ from: '/index.html', to: '/witforge/docs/' }],
      [{ from: '/.html', to: '/witforge/docs/' }],
      [{ from: '/guide/.html', to: '/witforge/docs/' }],
      [{ from: '/guide/../escape.html', to: '/witforge/docs/' }],
      [{ from: '/D:/escape.html', to: '/witforge/docs/' }],
      [{ from: '/guide/no-ext', to: '/witforge/docs/' }],
      [{ from: '/guide/a.html', to: '/docs/' }],
      [{ from: '/guide/a.html', to: '/witforge/docs' }],
      [{ from: '/guide/a.html', to: '/witforge/docs/', title: '   ' }],
      [
        { from: '/guide/dup.html', to: '/witforge/docs/' },
        { from: '/guide/DUP.html', to: '/witforge/docs/quickstart/' },
      ],
      [
        { from: '/guide/a.html', to: '/witforge/docs/' },
        { from: '/guide/a.html/nested.html', to: '/witforge/docs/quickstart/' },
      ],
    ];
    for (const [idx, payload] of badCases.entries()) {
      const file = join(scratchDir, `bad-${idx}.json`);
      writeFileSync(file, JSON.stringify(payload), 'utf8');
      assert.throws(() => loadRedirects(file), `Expected case #${idx} to fail validation`);
    }

    const validFile = join(scratchDir, 'ok.json');
    writeFileSync(validFile, JSON.stringify([{ from: '/a.html', to: '/witforge/docs/' }]), 'utf8');
    assert.throws(() => loadRedirects(validFile, { basePath: 'witforge' }), /Invalid basePath/);
  } finally {
    rmSync(scratchDir, { recursive: true, force: true });
  }
});

test('findProseIssuesInHtml and findProseIssuesInPlainText catch em dashes, en dashes, clichés, and table leaks', () => {
  const cleanHtml =
    '<html><head><meta name="description" content="Direct engineering prose."></head>' +
    '<body><pre><code>seamlessly — delve</code></pre><table><tr><td><code>--format json</code></td></tr></table>' +
    '<p>Clear sentence with zero clichés.</p></body></html>';
  assert.deepEqual(findProseIssuesInHtml(cleanHtml), []);

  const dirtyHtml = [
    '<html><head><meta property="og:description" content="Supercharge your contracts"></head>',
    '<body>',
    '<p>Bad em dash \u2014 here and en dash \u2013 here.</p>',
    '<p>We seamlessly delve into cutting-edge tooling.</p>',
    '<table><tr><td>Added <code>witforge diff</code>, `--format json</td></tr></table>',
    '</body></html>',
  ].join('\n');
  const htmlIssues = findProseIssuesInHtml(dirtyHtml);
  assert.ok(htmlIssues.some((msg) => msg.includes('em dash')));
  assert.ok(htmlIssues.some((msg) => msg.includes('en dash')));
  assert.ok(htmlIssues.some((msg) => msg.includes('leaked backtick in table cell')));
  assert.ok(htmlIssues.some((msg) => msg.includes('Supercharge')));
  assert.ok(htmlIssues.some((msg) => msg.includes('seamlessly')));
  assert.ok(htmlIssues.some((msg) => msg.includes('delve')));
  assert.ok(htmlIssues.some((msg) => msg.includes('cutting-edge')));

  const cleanText = '# Docs\n\n```sh\n# seamlessly delve\n```\n\nDirect guide text.\n';
  assert.deepEqual(findProseIssuesInPlainText(cleanText), []);

  const dirtyText = 'Overview \u2014 unlock the power of WIT.\n';
  const textIssues = findProseIssuesInPlainText(dirtyText);
  assert.ok(textIssues.some((msg) => msg.includes('em dash')));
  assert.ok(textIssues.some((msg) => msg.includes('unlock the power')));
});

test('validateSvgDiagramsInHtml accepts valid diagrams and catches missing metadata or out-of-bounds shapes', () => {
  const validSvg = [
    '<figure>',
    '<svg viewBox="0 0 200 100" role="img" aria-labelledby="t d">',
    '<title id="t">Valid diagram</title>',
    '<desc id="d">A box inside bounds.</desc>',
    '<rect x="10" y="10" width="180" height="80" />',
    '<text x="100" y="50">OK</text>',
    '<path d="M10 50 L190 50" />',
    '</svg>',
    '</figure>',
    // Small decorative UI icon must be ignored
    '<svg width="16" height="16" viewBox="0 0 24 24"><path d="M0 0 L24 24" /></svg>',
  ].join('\n');
  const ok = validateSvgDiagramsInHtml(validSvg, 'index.html');
  assert.equal(ok.diagramCount, 1);
  assert.deepEqual(ok.errors, []);

  const badSvg = [
    '<figure>',
    '<svg viewBox="0 0 200 100" aria-labelledby="missing-id">',
    '<rect x="10" y="10" width="250" height="80" />',
    '<path d="M0 50 L250 50" />',
    '</svg>',
    '</figure>',
  ].join('\n');
  const bad = validateSvgDiagramsInHtml(badSvg, 'bad.html');
  assert.equal(bad.diagramCount, 1);
  assert.ok(bad.errors.some((e) => e.includes('missing role="img"')));
  assert.ok(bad.errors.some((e) => e.includes('missing id "missing-id"')));
  assert.ok(bad.errors.some((e) => e.includes('missing non-empty <title>')));
  assert.ok(bad.errors.some((e) => e.includes('missing non-empty <desc>')));
  assert.ok(bad.errors.some((e) => e.includes('outside SVG viewBox')));

  const badViewBox = '<svg role="img" viewBox="0 0 -10 100" aria-labelledby="a"><title id="a">T</title><desc>D</desc></svg>';
  const badVb = validateSvgDiagramsInHtml(badViewBox, 'vb.html');
  assert.ok(badVb.errors.some((e) => e.includes('invalid or missing numeric viewBox')));
});

test('checkSite catches broken links, missing trailing slashes, missing fragments, bad GitHub links, and drifted examples', () => {
  const scratchRoot = makeTempDir('witforge-check-links-test-');
  try {
    const dist = join(scratchRoot, 'dist');
    const docs = join(scratchRoot, 'docs');
    const manifest = join(scratchRoot, 'redirects.json');
    mkdirSync(join(dist, 'docs', 'diagnostics'), { recursive: true });
    mkdirSync(join(dist, 'pagefind', 'fragment'), { recursive: true });
    mkdirSync(join(docs, 'guide'), { recursive: true });
    mkdirSync(join(scratchRoot, 'examples', 'single'), { recursive: true });
    mkdirSync(join(scratchRoot, 'examples', 'evolved'), { recursive: true });

    writeFileSync(join(scratchRoot, 'Cargo.toml'), '[workspace.package]\nversion = "0.9.0"\n', 'utf8');
    writeFileSync(join(scratchRoot, 'examples', 'single', 'contract.wit'), 'package a:b@0.1.0;\n', 'utf8');
    writeFileSync(join(scratchRoot, 'examples', 'evolved', 'convert.rs'), 'pub fn convert() {}\n', 'utf8');
    writeFileSync(join(docs, 'guide', 'diagnostics.md'), '### WF1001\n\nBody.\n', 'utf8');
    writeFileSync(
      manifest,
      JSON.stringify([{ from: '/guide/diagnostics.html', to: '/witforge/docs/diagnostics/', title: 'Diagnostics' }]),
      'utf8',
    );

    const pngHeader = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00]);
    writeFileSync(join(dist, 'robots.txt'), 'Sitemap: https://asamarts.github.io/witforge/sitemap-index.xml\n', 'utf8');
    writeFileSync(
      join(dist, 'sitemap-index.xml'),
      '<sitemapindex><sitemap><loc>https://asamarts.github.io/witforge/sitemap-0.xml</loc></sitemap></sitemapindex>\n',
      'utf8',
    );
    writeFileSync(join(dist, 'sitemap-0.xml'), '<urlset><url><loc>https://asamarts.github.io/witforge/</loc></url></urlset>\n', 'utf8');
    writeFileSync(join(dist, '404.html'), '<!DOCTYPE html><html><body><p>Not found.</p></body></html>\n', 'utf8');
    writeFileSync(join(dist, 'favicon.svg'), '<svg></svg>\n', 'utf8');
    writeFileSync(join(dist, 'og-card.svg'), '<svg></svg>\n', 'utf8');
    writeFileSync(join(dist, 'og-card.png'), pngHeader);
    writeFileSync(join(dist, 'llms.txt'), '# witforge v0.9.0\n', 'utf8');
    writeFileSync(join(dist, 'llms-full.txt'), '# witforge v0.9.0\n', 'utf8');
    writeFileSync(join(dist, 'pagefind', 'pagefind.js'), '// stub\n', 'utf8');
    writeFileSync(join(dist, 'pagefind', 'pagefind-entry.json'), '{}\n', 'utf8');
    writeFileSync(join(dist, 'pagefind', 'fragment', 'en_1.pf_fragment'), 'data\n', 'utf8');

    const validIndex = [
      '<!DOCTYPE html><html><body>',
      '<p>witforge v0.9.0</p>',
      '<a href="/witforge/docs/diagnostics/#wf1001">WF1001</a>',
      '<figure><svg viewBox="0 0 240 100" role="img" aria-labelledby="t1 d1"><title id="t1">Diagram</title><desc id="d1">Desc</desc><rect x="10" y="10" width="200" height="80"/></svg></figure>',
      '<section><div><div>',
      '<div id="tour-panel-wit"><a href="https://github.com/asamarts/witforge/blob/main/examples/single/contract.wit">src</a><pre><code>package a:b@0.1.0;</code></pre></div>',
      '<div id="tour-panel-convert"><a href="https://github.com/asamarts/witforge/blob/main/examples/evolved/convert.rs">src</a><pre><code>pub fn convert() {}</code></pre></div>',
      '</div></div></section>',
      '</body></html>',
    ].join('\n');
    writeFileSync(join(dist, 'index.html'), validIndex, 'utf8');
    writeFileSync(
      join(dist, 'docs', 'diagnostics', 'index.html'),
      '<!DOCTYPE html><html><body><h3 id="wf1001">WF1001</h3></body></html>\n',
      'utf8',
    );
    emitRedirects({ distDir: dist, manifestPath: manifest });

    // Positive run must succeed with zero errors
    const ok = checkSite({
      distDir: dist,
      repoRoot: scratchRoot,
      docsRoot: docs,
      manifestPath: manifest,
      minDiagrams: 1,
    });
    assert.deepEqual(ok.errors, []);
    assert.equal(ok.stats.verifiedDiagnosticCodes, 1);
    assert.equal(ok.stats.verifiedTourPanels, 2);

    // Negative run: inject broken link, missing trailing slash, missing fragment, bad repo link,
    // em dash, cliché, drifted tour excerpt, and missing WF1001 anchor
    const brokenIndex = validIndex
      .replace('/witforge/docs/diagnostics/#wf1001', '/witforge/docs/diagnostics#missing-frag')
      .replace(
        '</body>',
        '<a href="/witforge/nope/">404</a><a href="https://github.com/asamarts/witforge/blob/main/no/such/file.rs">bad</a><p>Bad \u2014 seamlessly</p></body>',
      )
      .replace('package a:b@0.1.0;', 'package drifted:code@0.1.0;');
    writeFileSync(join(dist, 'index.html'), brokenIndex, 'utf8');
    writeFileSync(
      join(dist, 'docs', 'diagnostics', 'index.html'),
      '<!DOCTYPE html><html><body><h3>WF1001</h3></body></html>\n',
      'utf8',
    );

    const bad = checkSite({
      distDir: dist,
      repoRoot: scratchRoot,
      docsRoot: docs,
      manifestPath: manifest,
      minDiagrams: 1,
    });
    assert.ok(bad.errors.some((e) => e.includes('missing trailing slash')));
    assert.ok(bad.errors.some((e) => e.includes('broken fragment "#missing-frag"')));
    assert.ok(bad.errors.some((e) => e.includes('broken internal link "/witforge/nope/"')));
    assert.ok(bad.errors.some((e) => e.includes('GitHub repo link target does not exist')));
    assert.ok(bad.errors.some((e) => e.includes('em dash')));
    assert.ok(bad.errors.some((e) => e.includes('banned prose cliché')));
    assert.ok(bad.errors.some((e) => e.includes('excerpt drifted')));
    assert.ok(bad.errors.some((e) => e.includes('missing lowercase anchor id="wf1001"')));
  } finally {
    rmSync(scratchRoot, { recursive: true, force: true });
  }
});
