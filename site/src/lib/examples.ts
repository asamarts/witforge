import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import type { BuiltinLanguage } from 'shiki';
import { REPO_ROOT, REPO_URL } from './site';

export interface OutputTourTab {
  id: string;
  label: string;
  lang: BuiltinLanguage | 'text';
  exampleGroup: 'single' | 'evolved';
  sourcePath: string;
  sourceUrl: string;
  caption: string;
  code: string;
}

function readRepoExcerpt(
  relPath: string,
  expectedMarker: string,
  startLine?: number,
  endLine?: number,
): string {
  const fullPath = resolve(REPO_ROOT, ...relPath.split('/'));
  const raw = readFileSync(fullPath, 'utf8').replace(/\r\n/g, '\n');
  const lines = raw.split('\n');
  const slice =
    startLine !== undefined && endLine !== undefined
      ? lines.slice(startLine - 1, endLine).join('\n').trimEnd()
      : raw.trimEnd();

  if (!slice.includes(expectedMarker)) {
    throw new Error(
      `Expected marker "${expectedMarker}" not found in ${relPath}` +
        (startLine !== undefined ? ` (lines ${startLine}-${endLine})` : ''),
    );
  }
  return slice;
}

export const OUTPUT_TOUR_TABS: OutputTourTab[] = [
  {
    id: 'wit',
    label: 'contract.wit',
    lang: 'wit',
    exampleGroup: 'single',
    sourcePath: 'examples/single/wit/v0.1/contract.wit',
    sourceUrl: `${REPO_URL}/blob/main/examples/single/wit/v0.1/contract.wit`,
    caption: 'Hand-written WIT package (example:ingest@0.1.0) defining records, enums, flags, variants, functions, and worlds.',
    code: readRepoExcerpt(
      'examples/single/wit/v0.1/contract.wit',
      'package example:ingest@0.1.0;',
    ),
  },
  {
    id: 'lock',
    label: 'wire.lock',
    lang: 'text',
    exampleGroup: 'single',
    sourcePath: 'examples/single/wit/v0.1/wire.lock',
    sourceUrl: `${REPO_URL}/blob/main/examples/single/wit/v0.1/wire.lock`,
    caption: 'Append-only v2 lockfile assigning stable wire numbers and declaration-order fingerprints.',
    code: readRepoExcerpt(
      'examples/single/wit/v0.1/wire.lock',
      '# witforge wire lockfile v2',
    ),
  },
  {
    id: 'rust',
    label: 'Plain Rust',
    lang: 'rust',
    exampleGroup: 'single',
    sourcePath: 'examples/single/generated/rust/v0_1.rs',
    sourceUrl: `${REPO_URL}/blob/main/examples/single/generated/rust/v0_1.rs`,
    caption: 'Owned Rust structs, enums, and interface traits with zero runtime dependencies and full rustdoc comments.',
    code: readRepoExcerpt(
      'examples/single/generated/rust/v0_1.rs',
      'pub trait Ingest',
    ),
  },
  {
    id: 'proto',
    label: 'Protobuf',
    lang: 'proto',
    exampleGroup: 'single',
    sourcePath: 'examples/single/generated/proto/v0.proto',
    sourceUrl: `${REPO_URL}/blob/main/examples/single/generated/proto/v0.proto`,
    caption: 'Proto3 messages, closed enums, request/response envelopes, and locked method IDs from wire.lock.',
    code: readRepoExcerpt(
      'examples/single/generated/proto/v0.proto',
      'enum IngestMethod',
    ),
  },
  {
    id: 'glue',
    label: 'Rust Glue',
    lang: 'rust',
    exampleGroup: 'single',
    sourcePath: 'examples/single/generated/glue/v0.rs',
    sourceUrl: `${REPO_URL}/blob/main/examples/single/generated/glue/v0.rs`,
    caption: 'Checked conversions between plain Rust and prost protobuf types (lines 1-46 of v0.rs).',
    code: readRepoExcerpt(
      'examples/single/generated/glue/v0.rs',
      'GlueError::MissingField',
      1,
      46,
    ),
  },
  {
    id: 'schema',
    label: 'JSON Schema',
    lang: 'json',
    exampleGroup: 'single',
    sourcePath: 'examples/single/generated/schema/v0_1.schema.json',
    sourceUrl: `${REPO_URL}/blob/main/examples/single/generated/schema/v0_1.schema.json`,
    caption: 'WIT-shaped JSON Schema definitions (Draft 7 default, opt-in Draft 2020-12) carrying WIT descriptions.',
    code: readRepoExcerpt(
      'examples/single/generated/schema/v0_1.schema.json',
      '"title": "example:ingest@0.1.0"',
    ),
  },
  {
    id: 'convert',
    label: 'Version Adapter',
    lang: 'rust',
    exampleGroup: 'evolved',
    sourcePath: 'examples/evolved/generated/rust/convert.rs',
    sourceUrl: `${REPO_URL}/blob/main/examples/evolved/generated/rust/convert.rs`,
    caption: 'Adjacent-version adapters from the two-version evolved example (example:tasks v0.1 <-> v0.2, lines 57-149).',
    code: readRepoExcerpt(
      'examples/evolved/generated/rust/convert.rs',
      'pub mod v0_2_to_v0_1',
      57,
      149,
    ),
  },
];
