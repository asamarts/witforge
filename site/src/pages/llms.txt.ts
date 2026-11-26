import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import type { APIRoute } from 'astro';
import {
  CRATES_CLI_URL,
  DOCS_ROOT,
  DOCS_RS_BUILD_URL,
  DOCS_RS_URL,
  REPO_URL,
  SITE_ORIGIN,
  SITE_URL,
  WORKSPACE_MSRV,
  WORKSPACE_VERSION,
  canonicalUrl,
} from '../lib/site';
import {
  docsRelPathToRoute,
  parseSummary,
  resolvePageDescription,
  rewriteMarkdownLinks,
  stripLeadingH1,
  stripYamlFrontmatter,
} from '../../scripts/markdown-utils.mjs';

const SITE_BASE_ABS = `${SITE_ORIGIN}/witforge`;

export const GET: APIRoute = () => {
  const summaryRaw = readFileSync(resolve(DOCS_ROOT, 'SUMMARY.md'), 'utf8');
  const groups = parseSummary(summaryRaw);

  const lines: string[] = [
    '# witforge',
    '',
    `> WIT-first contract compiler (v${WORKSPACE_VERSION}, Rust MSRV ${WORKSPACE_MSRV}, Apache-2.0 OR MIT). Compiles one hand-written WIT package with an append-only wire.lock into deterministic plain Rust types and traits, Protobuf schemas, JSON Schema (Draft 7 and Draft 2020-12), checked Rust/prost glue, Markdown contract docs, version adapters, and 4-view compatibility reports.`,
    '',
    'witforge is a contract compiler, not a WebAssembly runtime, RPC framework, or replacement for wit-bindgen or Wasmtime.',
    '',
    '## Key Links',
    '',
    `- [Homepage](${SITE_URL}): Overview, compiler pipeline, and verified output tour`,
    `- [Outputs and Evolution Matrix](${canonicalUrl('/outputs/')}): WIT type mappings and 4-view diff compatibility rules`,
    `- [Documentation Overview](${canonicalUrl('/docs/')}): Complete user guide index`,
    `- [Full Documentation Corpus](${SITE_BASE_ABS}/llms-full.txt): All published guides concatenated as plain Markdown`,
    `- [GitHub Repository](${REPO_URL}): Source code, examples, and design specifications`,
    `- [witforge API Docs](${DOCS_RS_URL}): Rust library API reference on docs.rs`,
    `- [witforge-build API Docs](${DOCS_RS_BUILD_URL}): build.rs helper crate reference on docs.rs`,
    `- [witforge-cli on crates.io](${CRATES_CLI_URL}): CLI binary crate`,
    '',
  ];

  for (const group of groups) {
    lines.push(`## ${group.label}`, '');
    for (const item of group.items) {
      const raw = readFileSync(resolve(DOCS_ROOT, ...item.relPath.split('/')), 'utf8');
      const { body: withoutFm } = stripYamlFrontmatter(raw);
      const { body: withoutH1 } = stripLeadingH1(withoutFm);
      const rewritten = rewriteMarkdownLinks(withoutH1, item.relPath, SITE_BASE_ABS);
      const description = resolvePageDescription(item.relPath, rewritten, item.label);
      const url = docsRelPathToRoute(item.relPath, SITE_BASE_ABS);
      lines.push(`- [${item.label}](${url}): ${description}`);
    }
    lines.push('');
  }

  return new Response(`${lines.join('\n').trimEnd()}\n`, {
    headers: {
      'Content-Type': 'text/plain; charset=utf-8',
    },
  });
};
