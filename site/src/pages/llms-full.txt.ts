import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import type { APIRoute } from 'astro';
import { DOCS_ROOT, SITE_ORIGIN, SITE_URL, WORKSPACE_MSRV, WORKSPACE_VERSION } from '../lib/site';
import {
  docsRelPathToRoute,
  parseSummary,
  rewriteMarkdownLinks,
  stripLeadingH1,
  stripYamlFrontmatter,
} from '../../scripts/markdown-utils.mjs';

const SITE_BASE_ABS = `${SITE_ORIGIN}/witforge`;

export const GET: APIRoute = () => {
  const summaryRaw = readFileSync(resolve(DOCS_ROOT, 'SUMMARY.md'), 'utf8');
  const groups = parseSummary(summaryRaw);

  const sections: string[] = [
    [
      '# witforge Full Documentation',
      '',
      `Version: ${WORKSPACE_VERSION}`,
      `Rust MSRV: ${WORKSPACE_MSRV}`,
      `Site: ${SITE_URL}`,
      `Index: ${SITE_BASE_ABS}/llms.txt`,
    ].join('\n'),
  ];

  for (const group of groups) {
    for (const item of group.items) {
      const raw = readFileSync(resolve(DOCS_ROOT, ...item.relPath.split('/')), 'utf8');
      const { body: withoutFm } = stripYamlFrontmatter(raw);
      const { body: withoutH1 } = stripLeadingH1(withoutFm);
      const rewritten = rewriteMarkdownLinks(withoutH1, item.relPath, SITE_BASE_ABS).trim();
      const canonical = docsRelPathToRoute(item.relPath, SITE_BASE_ABS);

      sections.push(
        [
          `<!-- Section: ${group.label} -->`,
          `# ${item.label}`,
          '',
          `URL: ${canonical}`,
          '',
          rewritten,
        ].join('\n'),
      );
    }
  }

  return new Response(`${sections.join('\n\n---\n\n')}\n`, {
    headers: {
      'Content-Type': 'text/plain; charset=utf-8',
    },
  });
};
