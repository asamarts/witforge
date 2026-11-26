import { existsSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

function findRepoRoot(): string {
  const starts = [process.cwd(), dirname(fileURLToPath(import.meta.url))];
  for (const start of starts) {
    let dir = resolve(start);
    while (true) {
      if (existsSync(resolve(dir, 'Cargo.toml')) && existsSync(resolve(dir, 'docs'))) {
        return dir;
      }
      const parent = dirname(dir);
      if (parent === dir) break;
      dir = parent;
    }
  }
  throw new Error('Could not locate witforge repository root containing Cargo.toml and docs/');
}

export const REPO_ROOT = findRepoRoot();
export const DOCS_ROOT = resolve(REPO_ROOT, 'docs');
const WORKSPACE_CARGO_TOML = resolve(REPO_ROOT, 'Cargo.toml');

interface WorkspaceMetadata {
  version: string;
  msrv: string;
}

const WORKSPACE_PACKAGE_HEADER_RE = /^\[\s*workspace\.package\s*\](?:\s*#.*)?$/;
const SCHEME_OR_PROTOCOL_RE = /^(?:[a-z][a-z0-9+.-]*:|\/\/)/i;

function readWorkspaceMetadata(): WorkspaceMetadata {
  const raw = readFileSync(WORKSPACE_CARGO_TOML, 'utf8');
  let inWorkspacePackage = false;
  let version: string | null = null;
  let msrv: string | null = null;

  for (const line of raw.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (trimmed.startsWith('[')) {
      inWorkspacePackage = WORKSPACE_PACKAGE_HEADER_RE.test(trimmed);
      continue;
    }
    if (!inWorkspacePackage) continue;

    const versionMatch = trimmed.match(/^version\s*=\s*"([^"]+)"/);
    if (versionMatch) {
      version = versionMatch[1];
      continue;
    }

    const msrvMatch = trimmed.match(/^rust-version\s*=\s*"([^"]+)"/);
    if (msrvMatch) {
      msrv = msrvMatch[1];
    }
  }

  if (!version) {
    throw new Error(`Could not find [workspace.package] version in ${WORKSPACE_CARGO_TOML}`);
  }
  if (!msrv) {
    throw new Error(`Could not find [workspace.package] rust-version in ${WORKSPACE_CARGO_TOML}`);
  }

  return { version, msrv };
}

const workspace = readWorkspaceMetadata();

export const SITE_TITLE = 'witforge';
export const SITE_TAGLINE = 'One WIT contract. Every stable projection.';
export const SITE_DESCRIPTION =
  'WIT-first contract compiler that checks a portable WIT subset, locks wire numbers, and emits deterministic Rust, Protobuf, JSON Schema, checked glue, Markdown docs, and version adapters.';

export const SITE_ORIGIN = 'https://asamarts.github.io';
export const BASE_PATH = '/witforge';
export const SITE_URL = `${SITE_ORIGIN}${BASE_PATH}/`;
export const OG_IMAGE_URL = `${SITE_ORIGIN}${BASE_PATH}/og-card.png`;

export const WORKSPACE_VERSION = workspace.version;
export const WORKSPACE_MSRV = workspace.msrv;

export const REPO_URL = 'https://github.com/asamarts/witforge';
export const RELEASE_TAG_URL = `${REPO_URL}/releases/tag/v${WORKSPACE_VERSION}`;
export const ISSUES_URL = `${REPO_URL}/issues`;
export const CRATES_CLI_URL = 'https://crates.io/crates/witforge-cli';
export const CRATES_LIB_URL = 'https://crates.io/crates/witforge';
export const CRATES_BUILD_URL = 'https://crates.io/crates/witforge-build';
export const DOCS_RS_URL = 'https://docs.rs/witforge/latest/witforge/';
export const DOCS_RS_BUILD_URL = 'https://docs.rs/witforge-build/latest/witforge_build/';

export const THEME_COLOR_LIGHT = '#ffffff';
export const THEME_COLOR_DARK = '#0a0f1a';

/**
 * Prefix a site-relative path with BASE_PATH (`/witforge`).
 * Preserves already-prefixed paths, scheme URLs (http/https/mailto/tel),
 * protocol-relative URLs, and query/hash-only links.
 */
export function withBase(path: string): string {
  if (!path) {
    return `${BASE_PATH}/`;
  }
  if (path.startsWith('#') || path.startsWith('?') || SCHEME_OR_PROTOCOL_RE.test(path)) {
    return path;
  }
  if (path === BASE_PATH) {
    return `${BASE_PATH}/`;
  }
  if (path.startsWith(`${BASE_PATH}?`) || path.startsWith(`${BASE_PATH}#`)) {
    return `${BASE_PATH}/${path.slice(BASE_PATH.length)}`;
  }
  if (path.startsWith(`${BASE_PATH}/`)) {
    return path;
  }
  const normalized = path.startsWith('/') ? path : `/${path}`;
  return `${BASE_PATH}${normalized}`;
}

/**
 * Build an absolute canonical URL for a site-relative or absolute path.
 */
export function canonicalUrl(path: string): string {
  if (/^https?:\/\//i.test(path)) {
    return new URL(path).href;
  }
  if (!path || path.startsWith('#') || path.startsWith('?')) {
    return SITE_URL;
  }
  const cleanPath = path.split(/[?#]/, 1)[0] || '/';
  return new URL(withBase(cleanPath), SITE_ORIGIN).href;
}
