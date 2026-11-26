export interface TypeMappingRow {
  wit: string;
  rust: string;
  proto: string;
  schema: string;
  glueAndNotes: string;
}

export interface TypeMappingGroup {
  id: string;
  label: string;
  description: string;
  rows: TypeMappingRow[];
}

export type VerdictTone = 'ok' | 'err' | 'warn' | 'neutral';

export interface VerdictCell {
  label: string;
  tone: VerdictTone;
}

export interface EvolutionRow {
  change: string;
  wire: VerdictCell;
  source: VerdictCell;
  api: VerdictCell;
  decode: VerdictCell;
  convert: string;
}

export interface OptInProfile {
  flag: string;
  scope: string;
  summary: string;
}

export const TYPE_MAPPING_GROUPS: TypeMappingGroup[] = [
  {
    id: 'primitives',
    label: 'Primitives',
    description:
      'Scalar WIT types keep exact integer widths and signedness in Rust, use explicit proto3 presence when required, and enforce narrow ranges and Unicode scalar validity in checked glue.',
    rows: [
      {
        wit: 'bool',
        rust: 'bool',
        proto: 'optional bool (required) / bool in flags',
        schema: '{"type":"boolean"}',
        glueAndNotes: 'Required record fields reject absent proto optional with GlueError::MissingField.',
      },
      {
        wit: 'u8, u16, u32',
        rust: 'u8, u16, u32',
        proto: 'optional uint32',
        schema: 'integer with exact min/max (0..255, 0..65535, 0..4294967295)',
        glueAndNotes: 'Glue range-checks u8 and u16 from uint32 (GlueError::OutOfRange).',
      },
      {
        wit: 'u64',
        rust: 'u64',
        proto: 'optional uint64',
        schema: 'integer (0..18446744073709551615)',
        glueAndNotes: 'Proto3-JSON encodes u64 as decimal strings; Rust Serde emits exact JSON numbers.',
      },
      {
        wit: 's8, s16, s32',
        rust: 'i8, i16, i32',
        proto: 'optional sint32',
        schema: 'integer with exact signed min/max',
        glueAndNotes: 'ZigZag-encoded sint32; glue range-checks s8 and s16 into i8/i16.',
      },
      {
        wit: 's64',
        rust: 'i64',
        proto: 'optional sint64',
        schema: 'integer (-9223372036854775808..9223372036854775807)',
        glueAndNotes: 'ZigZag-encoded sint64; proto3-JSON encodes s64 as decimal strings.',
      },
      {
        wit: 'f32, f64',
        rust: 'f32, f64',
        proto: 'optional float / optional double',
        schema: 'number (f32 bounded to finite IEEE-754 range)',
        glueAndNotes: 'Types containing floats omit Eq and Hash even when --eq-hash is enabled.',
      },
      {
        wit: 'char',
        rust: 'char',
        proto: 'optional uint32',
        schema: '{"type":"string","minLength":1,"maxLength":1}',
        glueAndNotes: 'Glue validates Unicode scalar values via char::from_u32 (GlueError::InvalidChar).',
      },
      {
        wit: 'string',
        rust: '::std::string::String (::alloc::string::String under --no-std)',
        proto: 'optional string',
        schema: '{"type":"string"}',
        glueAndNotes: 'UTF-8 string; presence-checked when required.',
      },
    ],
  },
  {
    id: 'composites',
    label: 'Composites',
    description:
      'Named records, variants, enums, flags, options, and all four WIT result shapes map into exhaustive Rust types, locked protobuf declarations, and closed JSON Schema definitions.',
    rows: [
      {
        wit: 'record',
        rust: 'pub struct with pub fields',
        proto: 'message <Interface><Type>',
        schema: 'closed object (all properties required, additionalProperties: false)',
        glueAndNotes: 'Field numbers locked in wire.lock; required fields enforce presence in glue.',
      },
      {
        wit: 'variant',
        rust: 'pub enum with unit and tuple variants',
        proto: 'message with oneof witforge_value (Empty for payload-less cases)',
        schema: 'oneOf single-key closed objects (null for payload-less cases)',
        glueAndNotes: 'Unset oneof fails decode with GlueError::UnknownCase.',
      },
      {
        wit: 'enum',
        rust: 'pub enum (unit variants)',
        proto: 'enum with <ENUM>_UNSPECIFIED = 0 and locked case numbers',
        schema: '{"type":"string","enum":[...]} (WIT kebab-case spelling)',
        glueAndNotes: 'Closed enum: 0 and unknown values fail with GlueError::UnknownEnumValue.',
      },
      {
        wit: 'flags',
        rust: 'pub struct of pub bool fields',
        proto: 'message of bool fields (locked field numbers)',
        schema: 'closed object of required boolean properties',
        glueAndNotes: 'Absent proto3 bool decodes as false; adding/removing a flag stays binary decode compatible.',
      },
      {
        wit: 'option<T>',
        rust: '::core::option::Option<T>',
        proto: 'optional T, message presence, or WitforgeOptionOf... wrapper',
        schema: 'anyOf: [schema(T), {"type":"null"}] (property remains required)',
        glueAndNotes: 'Nested option<option<T>> and option<list/map> use synthesized wrapper messages.',
      },
      {
        wit: 'result<T, E> (all 4 forms)',
        rust: '::core::result::Result<T, E> (() for omitted ok/err)',
        proto: 'message with oneof value { ok = 1; err = 2; } (Empty when omitted)',
        schema: 'closed single-key object with "ok" or "err" (null when omitted)',
        glueAndNotes: 'Unset oneof fails decode with GlueError::UnknownCase.',
      },
    ],
  },
  {
    id: 'collections',
    label: 'Collections',
    description:
      'Byte lists, generic lists, fixed-length arrays, and primitive-keyed maps preserve deterministic ordering and explicit wire length and presence semantics.',
    rows: [
      {
        wit: 'list<u8> (or byte alias)',
        rust: '::std::vec::Vec<u8> (::alloc::vec::Vec<u8> under --no-std)',
        proto: 'optional bytes (when required or optional)',
        schema: 'array of 0..=255 integers (not base64)',
        glueAndNotes: 'Required list<u8> uses optional bytes presence (MissingField if absent on the wire).',
      },
      {
        wit: 'list<T>',
        rust: '::std::vec::Vec<T> (::alloc::vec::Vec<T> under --no-std)',
        proto: 'repeated singular(T) (wrapped inside oneof/option)',
        schema: '{"type":"array","items":...}',
        glueAndNotes: 'Absent repeated field decodes as an empty Vec; nested lists use wrapper messages.',
      },
      {
        wit: 'list<T, N> (1 <= N <= 65536)',
        rust: '[T; N] (private Serde helper generated when N > 32)',
        proto: 'optional bytes (for u8) or repeated singular(T)',
        schema: 'array with minItems: N and maxItems: N',
        glueAndNotes: 'Glue verifies exact length N on decode (GlueError::InvalidLength).',
      },
      {
        wit: 'map<K, V> (K: bool, int, char, string)',
        rust: '::std::collections::BTreeMap<K, V> (::alloc under --no-std)',
        proto: 'map<proto_key(K), singular(V)> or WitforgeMapOf... wrapper',
        schema: 'object with additionalProperties + propertyNames constraints',
        glueAndNotes: 'Deterministic BTreeMap order; char and narrow int keys validated in glue.',
      },
    ],
  },
  {
    id: 'imports',
    label: 'Dependencies and Aliases',
    description:
      'Standard deps/ packages and WIT type aliases stay deterministic across multi-version contract roots without duplicating type definitions.',
    rows: [
      {
        wit: 'type a = b and use aliases',
        rust: 'pub type A = B; inside the declaring/using interface module',
        proto: 'Inlined to the defining type (no extra message or lockfile key)',
        schema: '{"$ref":"#/definitions/..."} (or $defs in Draft 2020-12)',
        glueAndNotes: 'Byte-list aliases (type byte = u8; list<byte>) resolve to bytes/Vec<u8>.',
      },
      {
        wit: 'deps/ packages and worlds',
        rust: 'witforge_deps::<ns>_<pkg> (major-scoped only if majors differ) + worlds.rs',
        proto: '<ns>.<pkg>.v<major>.proto imported by root v<major>.proto',
        schema: '<ns>.<pkg>.v<major>.schema.json referenced across files',
        glueAndNotes: 'Unused unsupported dependency items are pruned cleanly; witforge deps sync keeps minors aligned.',
      },
    ],
  },
];

export const OPT_IN_PROFILES: OptInProfile[] = [
  {
    flag: '--serde',
    scope: 'Rust emitter',
    summary:
      'Adds plain #[derive(Serialize, Deserialize)] using Rust field names, plus private helpers for [T; N] with N > 32.',
  },
  {
    flag: '--no-std',
    scope: 'Rust, convert.rs, glue',
    summary:
      'Replaces ::std paths with ::core and ::alloc (including alloc::collections::BTreeMap and core::error::Error on Rust 1.88+).',
  },
  {
    flag: '--eq-hash',
    scope: 'Rust emitter',
    summary:
      'Derives Eq and Hash on every generated type that does not transitively contain f32 or f64.',
  },
  {
    flag: '--try-from',
    scope: 'Rust convert.rs',
    summary:
      'Emits TryFrom<&Old> and TryFrom<&New> impls delegating to the generated adjacent-version conversion functions.',
  },
  {
    flag: '--draft 2020-12',
    scope: 'JSON Schema emitter',
    summary:
      'Switches JSON Schema from Draft 7 (definitions) to Draft 2020-12 ($defs and native deprecated: true annotations).',
  },
];

const OK: VerdictCell = { label: 'Compatible', tone: 'ok' };
const BREAKING: VerdictCell = { label: 'Breaking', tone: 'err' };
const RESERVED: VerdictCell = { label: 'Compatible (reserved)', tone: 'warn' };

export const EVOLUTION_RULES: EvolutionRow[] = [
  {
    change: 'Add option<T>, non-byte list<T>, or map<K, V> field',
    wire: OK,
    source: BREAKING,
    api: BREAKING,
    decode: OK,
    convert: 'old -> new defaults (None / empty)',
  },
  {
    change: 'Add required list<u8> field',
    wire: OK,
    source: BREAKING,
    api: BREAKING,
    decode: { label: 'Breaking (new<-old)', tone: 'err' },
    convert: 'old -> new defaults (Vec::new()); wire optional bytes rejects absence',
  },
  {
    change: 'Add required scalar, string, char, list<T, N>, or record field',
    wire: OK,
    source: BREAKING,
    api: BREAKING,
    decode: { label: 'Breaking (new<-old)', tone: 'err' },
    convert: 'old -> new omitted (no default value)',
  },
  {
    change: 'Add enum or variant case',
    wire: OK,
    source: BREAKING,
    api: BREAKING,
    decode: { label: 'Breaking (old<-new)', tone: 'err' },
    convert: 'new -> old returns UnknownCase for new case',
  },
  {
    change: 'Remove option<T>, non-byte list<T>, or map<K, V> field',
    wire: RESERVED,
    source: BREAKING,
    api: BREAKING,
    decode: OK,
    convert: 'old -> new drops field; new -> old defaults',
  },
  {
    change: 'Remove required field (including list<u8>, list<T, N>, or char)',
    wire: RESERVED,
    source: BREAKING,
    api: BREAKING,
    decode: { label: 'Breaking (old<-new)', tone: 'err' },
    convert: 'new -> old returns MissingMember',
  },
  {
    change: 'Remove enum or variant case',
    wire: RESERVED,
    source: BREAKING,
    api: BREAKING,
    decode: { label: 'Breaking (new<-old)', tone: 'err' },
    convert: 'old -> new returns UnknownCase for removed case',
  },
  {
    change: 'Add or remove a flag member',
    wire: RESERVED,
    source: BREAKING,
    api: BREAKING,
    decode: OK,
    convert: 'Added flag defaults false; removed flag dropped',
  },
  {
    change: 'Add interface function',
    wire: OK,
    source: BREAKING,
    api: BREAKING,
    decode: { label: 'Breaking (old<-new)', tone: 'err' },
    convert: 'n/a (extends interface trait and method enum)',
  },
  {
    change: 'Retype field, change type kind, or renumber / reuse reserved tag',
    wire: BREAKING,
    source: BREAKING,
    api: BREAKING,
    decode: { label: 'Breaking (both)', tone: 'err' },
    convert: 'Omitted; belongs in a new major version',
  },
  {
    change: 'Update docs or stability/deprecation metadata (in a new version)',
    wire: OK,
    source: OK,
    api: OK,
    decode: OK,
    convert: 'Full round-trip (never edit a released version in place)',
  },
];
