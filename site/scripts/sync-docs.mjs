import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import {
  docsRelPathToContentFile,
  docsRelPathToSlug,
  parseSummary,
  resolvePageDescription,
  rewriteMarkdownLinks,
  stripLeadingH1,
  stripYamlFrontmatter,
  toPosix,
  yamlString,
} from './markdown-utils.mjs';

const SCRIPTS_DIR = dirname(fileURLToPath(import.meta.url));
const DEFAULT_SITE_ROOT = resolve(SCRIPTS_DIR, '..');
const DEFAULT_DOCS_ROOT = resolve(DEFAULT_SITE_ROOT, '..', 'docs');

const EDIT_DOCS_BASE = 'https://github.com/asamarts/witforge/edit/main/docs';

const EXCLUDED_TOP_LEVEL = new Set([
  '_config.yml',
  'aplan-pilot.md',
  'assets',
  'design',
  'SUMMARY.md',
]);

const EXCLUDED_REL_FILES = new Set(['guide/index.md']);

function collectPublishedMarkdown(rootDir, currentDir = rootDir, out = []) {
  for (const entry of readdirSync(currentDir, { withFileTypes: true })) {
    const fullPath = join(currentDir, entry.name);
    const rel = toPosix(relative(rootDir, fullPath));
    const top = rel.split('/')[0];
    if (EXCLUDED_TOP_LEVEL.has(top)) {
      continue;
    }
    if (entry.isDirectory()) {
      collectPublishedMarkdown(rootDir, fullPath, out);
    } else if (entry.isFile() && entry.name.endsWith('.md') && !EXCLUDED_REL_FILES.has(rel)) {
      out.push(rel);
    }
  }
  return out.sort();
}

/**
 * Synchronize `docs/` into Starlight content (`site/src/content/docs/docs/`)
 * and emit `site/src/data/sidebar.json` according to `docs/SUMMARY.md`.
 */
export function syncDocs({
  docsRoot = DEFAULT_DOCS_ROOT,
  siteRoot = DEFAULT_SITE_ROOT,
  urlPrefix = '/witforge',
} = {}) {
  const summaryPath = join(docsRoot, 'SUMMARY.md');
  if (!existsSync(summaryPath)) {
    throw new Error(`Missing docs/SUMMARY.md at ${summaryPath}`);
  }

  const summaryRaw = readFileSync(summaryPath, 'utf8');
  const groups = parseSummary(summaryRaw);

  const summaryPaths = new Set();
  for (const group of groups) {
    for (const item of group.items) {
      summaryPaths.add(item.relPath);
      const srcPath = join(docsRoot, ...item.relPath.split('/'));
      if (!existsSync(srcPath)) {
        throw new Error(`docs/SUMMARY.md references missing file: docs/${item.relPath}`);
      }
    }
  }

  const publishedOnDisk = collectPublishedMarkdown(docsRoot);
  const orphans = publishedOnDisk.filter((rel) => !summaryPaths.has(rel));
  if (orphans.length > 0) {
    throw new Error(
      `Published docs missing from docs/SUMMARY.md: ${orphans.map((p) => `docs/${p}`).join(', ')}`,
    );
  }

  const contentDocsDir = join(siteRoot, 'src', 'content', 'docs', 'docs');
  rmSync(contentDocsDir, { recursive: true, force: true });
  mkdirSync(contentDocsDir, { recursive: true });

  const sidebar = [];
  const writtenFiles = [];

  for (const group of groups) {
    const sidebarItems = [];
    for (const item of group.items) {
      const srcPath = join(docsRoot, ...item.relPath.split('/'));
      const raw = readFileSync(srcPath, 'utf8');
      const { body: withoutFm } = stripYamlFrontmatter(raw);
      const { body: withoutH1 } = stripLeadingH1(withoutFm);
      const rewrittenBody = rewriteMarkdownLinks(withoutH1, item.relPath, urlPrefix).trimEnd();
      const description = resolvePageDescription(item.relPath, rewrittenBody, item.label);
      const editUrl = `${EDIT_DOCS_BASE}/${item.relPath}`;

      const outContentRel = docsRelPathToContentFile(item.relPath);
      const outPath = join(siteRoot, 'src', 'content', 'docs', ...outContentRel.split('/'));
      mkdirSync(dirname(outPath), { recursive: true });

      const content = [
        '---',
        `title: ${yamlString(item.label)}`,
        `description: ${yamlString(description)}`,
        `editUrl: ${yamlString(editUrl)}`,
        '---',
        '',
        rewrittenBody,
        '',
      ].join('\n');

      writeFileSync(outPath, content, 'utf8');
      writtenFiles.push(outContentRel);

      sidebarItems.push({
        label: item.label,
        slug: docsRelPathToSlug(item.relPath),
      });
    }
    sidebar.push({
      label: group.label,
      items: sidebarItems,
    });
  }

  const sidebarPath = join(siteRoot, 'src', 'data', 'sidebar.json');
  mkdirSync(dirname(sidebarPath), { recursive: true });
  writeFileSync(sidebarPath, `${JSON.stringify(sidebar, null, 2)}\n`, 'utf8');

  return {
    pageCount: writtenFiles.length,
    groupCount: sidebar.length,
    writtenFiles,
    sidebar,
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const result = syncDocs();
  console.log(
    `Synced ${result.pageCount} documentation pages across ${result.groupCount} sidebar groups.`,
  );
}
