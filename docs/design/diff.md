# Contract diff and breaking-change reporting

Status: implemented 2026-11-05 (`W8-001`); endpoint ordering, missing-lock, and root/direct
declaration-order boundaries 2026-11-18 (`W10-026`); conversion-impact notes 2026-11-17
(`W11-036`); conversion blockers and API hardening 2026-11-18 (`W10-035`, `W10-036`); kind
changes and reused reserved numbers are wire-breaking 2026-11-19 (`W10-045`, `W10-048`); alias
spellings and retyped conversion blockers 2026-11-20 (`W10-057`); released-manifest and
dependency-alias endpoints 2026-11-20 (`W10-056`); Git-ref base comparison (`--git-base`)
2026-11-25 (`W11-043`). The 2026-11-18 through 2026-11-20 items
(reversed-endpoint rejection, missing-lock guidance, `conversion_blockers`, kind changes, reused
reserved numbers, alias spellings, released-manifest checks, and dependency-alias endpoints)
shipped in 0.6.0.

`witforge diff <old> <new>` (or `witforge diff --git-base <rev> [<path>]`) compares two contract
versions and reports what changed in four independent compatibility views:

- **wire identity** — the append-only lockfile's numbers, kinds, and canonical member types;
- **source** — the WIT/component type shape;
- **generated API** — the Rust types, traits, and functions the Rust emitter generates;
- **decode** — whether a newer reader accepts older messages (`new_reads_old`) and whether an
  older reader accepts newer messages (`old_reads_new`) under the protobuf **binary** decode
  profile (the generated Rust glue and the [non-Rust decode profile](../guide/non-rust.md)).

A change can be compatible in one view and breaking in another. For example, adding an
`option<T>` record field is wire-compatible and decode-compatible (the old data decodes as `None`
and older binary readers ignore the unknown field), while changing the component type and breaking
struct literals in the generated Rust API. Adding a required record field or a closed enum case is
wire-compatible (no number changed meaning) yet **decode-breaking** (the new reader rejects old
messages with `MissingField`, or the old reader rejects the new enum value with
`UnknownEnumValue`).

Root-package docs are not compared. Stability metadata changes are reported as
`stability-changed`; they alter generated annotations but not wire identity, the WIT component
shape, or generated type signatures. Dependency docs and stability remain part of the exact
pinned dependency graph described below.

## Selecting the compared versions

Each argument is one of:

- a version directory containing `wire.lock`, compared as that exact version; or
- a WIT root containing `v<major>.<minor>/` directories, in which case the highest version under
  the root is compared.

Both endpoints must declare the same `namespace:name`, and the older version must be passed
first. `diff` refuses a second endpoint whose package version is older than the first with exit
`2`; equal versions remain comparable. The selected version directories must have valid WIT, a
current lockfile, and a wire number for every live member; otherwise the command fails rather
than guessing.

A directory with WIT source, a `deps/` tree, or a `v<major>.<minor>` name but no `wire.lock` is
reported as a missing lockfile with `lock --first`/`--against` guidance (exit `2`), not treated
as a WIT root.

A direct version-directory endpoint applies the member-level lockfile freshness rules the
generators use: a stale kind, canonical type, draft entry, or removed member that is not
reserved is refused with exit `1` instead of being compared. A WIT-root endpoint additionally
loads every version through the generator-grade loader, which requires declaration-order
fingerprints to be current; a root with a stale order fingerprint is refused before comparison.
Direct endpoints deliberately accept that stale order so a reorder can be reported. The two
forms therefore agree about stale members, but a reorder recorded only in the order section is
reported by a direct endpoint and refused by root loading. A direct endpoint still refuses an
order line for an unknown parent or one naming a non-member, a subset, or the wrong live set:
that is structurally inconsistent with the contract, not merely stale. Missing order lines are
accepted by a direct endpoint and reported as a reorder when the source order differs.

A version-directory endpoint also reads the default `released` manifest beside it, as `check`
does: a malformed manifest, or a symlink in a listed released directory (its `contract.wit`,
`wire.lock`, or a `deps/` entry), is refused with exit `2`. A WIT-root endpoint uses the
generator-grade loader, which validates the root's manifest. There is no `--released` option;
each endpoint uses its own root's manifest, so two checkouts can be compared independently.

`diff` does not require the canonical lockfile rendering; `check` is the canonical-form gate.
`diff` requires current entries, not a particular byte layout or comment set.

### Comparing against a Git revision (`--git-base`)

`witforge diff --git-base <rev> [<path>]` (and the library entry point
`witforge::diff::compare_git_base`) compares a version directory or WIT root in the current working
tree against its committed tree at `<rev>` (for example `origin/main` or `HEAD~1`; when `<path>`
is omitted on the CLI, the configured project `root` from `witforge.toml` is used):

- **Safe temporary materialization:** witforge reads regular file blobs directly from Git's object
  database (`git ls-tree -r -z` + `git cat-file --batch`) into a private temporary directory under
  `std::env::temp_dir()` (respecting `TMPDIR`) and deletes that directory before returning, on both
  success and error paths. No `git archive`, `tar`, or working-tree checkout is performed. When
  `<path>` is a single version directory (`wit/v0.2`), its basename is preserved and any sibling
  `<parent>/released` manifest at `<rev>` is validated directly from Git (refusing symlinks,
  submodules, and non-regular-file entries with `WF9004`, and checking manifest syntax and listed
  released directories in the Git tree at `<rev>`) without materializing sibling directories into
  the temporary tree.
- **Symlink and submodule guards (`WF9004`, exit `2`):** symlinks or submodules anywhere in the
  Git tree at `<rev>` (including the target path itself or a sibling `released` manifest) or in the
  working-tree `<path>` are refused before comparison.
- **Shallow-clone detection (`WF9004`, exit `2`):** if `<rev>` cannot be resolved and the enclosing
  repository is a shallow clone, witforge refuses with `WF9004` and instructs the caller to fetch
  full history or the base ref (`fetch-depth: 0` on `actions/checkout`). Missing revisions in a
  non-shallow repository, leading-`-` revisions, or passing two positional paths alongside
  `--git-base` are usage errors (`WF9001`, exit `2`). Paths outside any Git repository fail with
  `WF9004`, and paths absent or non-directory at `<rev>` fail with `WF9002`.
- **Deterministic endpoint labels:** the old endpoint path and physical locations in human,
  `--json`, and `--format json|sarif` output are rewritten from the ephemeral temporary directory
  to `<rev>:<repo-relative-path>` (for example `origin/main:wit/v0.1`), so reports are
  deterministic and leak no temporary directory paths.
- All other `diff` rules — version ordering (equal versions allowed; downgrades refused), `--json`,
  `--format json|sarif`, `--check[=VIEWS]`, decode verdicts, conversion impacts, and cross-major
  omissions — apply unchanged.

Within one major (`old.major == new.major`), both endpoints must resolve the identical pinned
dependency graph: package identities and versions, interface/type/function inventories and order,
shapes and signatures, stability, and docs. `diff` refuses the first mismatch as a contract
violation instead of describing it as a compatible local change. Across a major boundary
(`old.major != new.major`), a new major may add, remove, change, or upgrade pinned dependencies;
types whose foreign dependency changed are reported in `conversion_omissions` below.

## Change categories

| Category | Meaning |
|---|---|
| `added` | A declaration exists only in the new version. |
| `added-defaulted` | An added record field has a type the generated old-to-new conversion can default: `option<T>`, `list<T>`, `map<K, V>`, or an alias or named definition of one. |
| `removed` | A declaration exists only in the old version. |
| `reordered` | Common members changed declaration order. The report lists the old and new relative order of the common members. |
| `retyped` | A member, alias shape, or function result kept its name but changed type. |
| `kind-changed` | A named type kept its name but changed kind, such as record to enum. |
| `renumbered` | A common member kept its kind and type but changed wire number, including a function's method id. This is reported separately from a retype. |
| `stability-changed` | An existing interface, type, function, world, or world import/export changed `@since`, `@unstable`, or `@deprecated` metadata. `from` and `to` carry canonical metadata strings. |

Declaration order is compared for record fields, variant cases, enum cases, flags, functions,
function parameters, and world imports and exports. Additions and removals are ignored when
comparing the relative order of common members, as in the lockfile's order fingerprints.

## Compatibility rules

### Wire identity

Wire-breaking changes are:

- a changed wire number (`renumbered`), for a field, case, flag, parameter, or function method
  id;
- a new member within the same major whose number was reserved or assigned to a different live
  member in the old lockfile, even though the member itself is an addition;
- a changed kind or canonical type of a numbered member (`retyped`);
- a type kind change (`kind-changed`), whether or not the old numbers stay reserved: every
  reference to the type keeps its canonical name while its encoding changes between an enum
  value, a bool field, and a message;
- removal of a member or a whole type, interface, or function whose old lockfile entries are not
  retained with their numbers and reserved;
- deletion, un-reservation, renumbering, or retyping of a `reserved` lockfile entry within the
  same major (`reserved entry`), even when the WIT source did not change. Across a major boundary
  (`old.major != new.major`), each major is an independent wire namespace and a fresh lockfile
  may drop prior reserved entries.

Additions are wire-compatible: they receive new numbers. A removal is wire-compatible only when
the new lockfile keeps the old entry reserved. The lockfile remains the authority; `diff` does
not write it.

### Source

The WIT/component view is stricter than protobuf. Adding, removing, retyping, or reordering any
member that participates in a component type is source-breaking, including a defaulted field.
Adding a new type or interface is additive. Removing a type or interface, or changing a world's
imports, exports, or their order, is source-breaking.

### Generated API

The generated Rust API view tracks the exhaustive shapes in `rust` output:

- adding or removing a record field, variant case, enum case, flag, function, or function
  parameter is API-breaking;
- retyping a member, function result, or named type shape is API-breaking;
- reordering function parameters is API-breaking because parameters are positional;
- reordering fields, cases, flags, or functions is not API-breaking (the generated names and
  Rust type shapes stay the same);
- adding a new type or interface is additive; removing one is API-breaking.
- changing stability or deprecation metadata is non-breaking in all compatibility views; the
  generated declaration keeps the same name and type while consumer-facing annotations change.

These rules deliberately do not run `rustc` or compare generated files. They describe the
contract-level API surface, and `--check` is a gate, not a semantic-versioning replacement.

### Decode compatibility (protobuf binary)

The **decode** view answers two directional questions for every change between two versions
sharing **one major's** protobuf package, under **protobuf binary** encoding and the documented
decode profile ([`non-rust.md`](../guide/non-rust.md), [`emitters.md`](emitters.md) section 7):

- `new_reads_old`: whether a reader compiled against the newer version accepts every protobuf
  binary message produced by a writer at the older version;
- `old_reads_new`: whether a reader compiled against the older version accepts every protobuf
  binary message produced by a writer at the newer version.

A change is `decode_breaking` when either `new_reads_old` or `old_reads_new` is `false`.

| Change | `new_reads_old` | `old_reads_new` | Observed decode-profile behavior (binary) |
|---|---|---|---|
| Added `option<T>`, non-byte `list<T>`, or `map<K, V>` record field / parameter | `true` | `true` | Absence decodes as `None` / empty `Vec` / empty `BTreeMap`; old binary reader skips unknown field |
| Added required record field / parameter (including required `list<u8>`, `list<T, N>`, or `char`) | `false` | `true` | New reader rejects absent field (`MissingField` or `InvalidLength`); old binary reader skips unknown field |
| Removed `option<T>`, non-byte `list<T>`, or `map<K, V>` record field / parameter | `true` | `true` | New binary reader skips unknown field; old reader decodes absence as `None` / empty `Vec` / empty `BTreeMap` |
| Removed required record field / parameter (including required `list<u8>`, `list<T, N>`, or `char`) | `true` | `false` | New binary reader skips unknown field; old reader rejects absent field (`MissingField` or `InvalidLength`) |
| Added enum or variant case | `true` | `false` | New reader accepts all old cases; old reader rejects the new case (`UnknownEnumValue` / `UnknownCase`) |
| Removed enum or variant case | `false` | `true` | New reader rejects the removed case (`UnknownEnumValue` / `UnknownCase`); old reader accepts remaining cases |
| Added or removed flag | `true` | `true` | Flags are proto3 `bool` fields (absent defaults to `false`, unknown bool skipped in binary) |
| Added function (or added interface with functions) | `true` | `false` | New server accepts all old methods; old server rejects the added method id (`UnknownEnumValue`) |
| Removed function (or removed non-empty interface / type) | `false` | `true` | New reader no longer accepts the removed method/type; old reader accepts remaining ones |
| Added standalone type, type-only interface, or world change | `true` | `true` | No existing wire message fails to decode |
| Reordered record fields, cases, flags, functions, or parameters | `true` | `true` | Protobuf matches members by field number, not declaration order |
| Retyped, kind-changed, renumbered, or reused reserved/live number (same major) | `false` | `false` | Wire encoding or field-number identity disagrees in both directions |
| Stability or deprecation metadata changed | `true` | `true` | Generated annotations change; protobuf field numbers and value encodings do not |

Important boundaries of this view:

- **Single major:** the decode view describes two endpoints sharing one major version's protobuf
  package (`v<major>.proto`). Two majors have two distinct protobuf packages negotiated separately
  at handshake.
- **Binary vs proto3-JSON:** this view describes **protobuf binary** encoding, where unknown field
  numbers are silently skipped by older/newer readers. Strict proto3-JSON parsers (including
  `pbjson`'s generated `Deserialize` impls by default) reject unknown JSON field names, so even an
  added `option<T>` field fails `old_reads_new` over strict proto3-JSON unless unknown fields are
  ignored.
- **Distinct from `convert.rs`:** `convert.rs` converts plain Rust values inside one process (and
  defaults `list<u8>` to empty `Vec::new()`). The decode view describes two separate processes
  communicating over the wire (where required `list<u8>` is proto3 `optional bytes`, whose absence
  returns `MissingField`).

## Output

The human report has one line per change:

```text
retyped           record field       types/task.id                    number=1   wire:BREAKING source:BREAKING api:BREAKING decode:BREAKING(both)  u8 -> u64
added-defaulted   record field       types/task.note                  number=3   wire:compatible source:BREAKING api:BREAKING decode:compatible
summary: 2 change(s), 2 breaking; wire identity: 1, source: 2, generated API: 2, decode: 1 (new<-old: 1, old<-new: 1)
result: BREAKING
```

`--json` emits the same information as JSON (gaining decode fields without removing any existing
fields):

```json
{
  "old": { "path": "wit/v0.1", "package": "example:tasks", "version": "0.1.0" },
  "new": { "path": "wit/v0.2", "package": "example:tasks", "version": "0.2.0" },
  "changes": [
    {
      "kind": "retyped",
      "element": "record field",
      "path": "types/task.id",
      "number": 1,
      "from": "u8",
      "to": "u64",
      "wire_breaking": true,
      "source_breaking": true,
      "api_breaking": true,
      "decode_breaking": true,
      "new_reads_old": false,
      "old_reads_new": false
    }
  ],
  "summary": {
    "changes": 1,
    "breaking": 1,
    "wire_breaking": 1,
    "source_breaking": 1,
    "api_breaking": 1,
    "decode_breaking": 1,
    "new_reads_old_breaking": 1,
    "old_reads_new_breaking": 1
  },
  "conversion_impacts": [],
  "conversion_blockers": [],
  "conversion_omissions": []
}
```

`added` changes also carry `defaulted`; `reordered` changes carry `before` and `after`;
`renumbered` changes carry numeric `from` and `to`; `stability-changed` carries canonical string
`from` and `to` values such as `stable(since=0.1.0)` and
`deprecated(since=0.1.0,version=0.2.0)`. A change's `number` is the new member's wire number when
the change concerns a numbered member, or the old number for a removal.

`--check` (equivalent to `--check=wire,source,api`, or `[project.<name>.policy].diff-check` when
configured in `witforge.toml`) exits `0` when no change breaks the gated views and `1` when at
least one does; **`decode` is not part of the built-in default `--check` set**
and only gates when explicitly selected via `--check=<views>` or `policy.diff-check`. `--check=<views>` restricts or extends the gate to any
comma-separated subset of `wire`, `source`, `api`, and `decode` (for example `--check=wire,decode`
to allow source/API-breaking minor additions that remain wire- and binary-decode-compatible while
rejecting added required fields or closed enum/variant cases, or `--check=decode` to gate on
decode compatibility alone). An empty view set (`--check=`) or an unknown view name is a usage
error (`WF9001`, exit `2`). The GitHub Action's `diff-check` input maps `'true'` onto `--check`,
`'false'` onto omitting the flag, and any view list onto `--check=<views>`. A plain `diff` exits
`0` after reporting even a breaking change. With `--quiet` (`-q`), human output omits headers,
notes, and the `summary:` / `result:` lines while keeping errors and exit codes unchanged (JSON
and SARIF output are unaffected). Input, I/O, and parse errors use the normal `witforge` exit
conventions: `1` for contract/lockfile violations and `2` for usage, I/O, and parse failures.

## Conversion impact notes

A shared record type can change so that the older version requires a member the newer version
removed. When that member's type has no default, a newer-to-older conversion cannot reconstruct
it and returns `ConversionError::MissingMember` for every input; for adjacent versions this is
the generated `convert.rs` adapter when the emitter can generate the pair. `diff` reports one
informational note per affected record type, naming the newer-to-older direction and the first
missing member in the older type's declaration order, which is the member the conversion reports
first.

```text
note: new-to-old conversion for `types/task` (0.2.0 -> 0.1.0) always fails: missing required member `types/task.old`
```

`--json` carries the notes in a separate array, and `--format json|sarif` emits each one as a
`WF3003` note:

```json
"conversion_impacts": [
  {
    "type_path": "types/task",
    "from_version": "0.2.0",
    "to_version": "0.1.0",
    "missing_member": "old",
    "missing_path": "types/task.old"
  }
]
```

Notes are not changes: they do not affect `changes`, `summary`, compatibility verdicts, or exit
codes. A removed variant or enum case is not a guaranteed failure because only values that use
the removed case fail; that remains a value-dependent `UnknownCase` path. Defaultable removals
(`option<T>`, `list<T>`, or a named alias or structural type of one) are reconstructed and have
no note.

An added record field without a default has the opposite generation consequence: the newer
version requires a value the older version cannot supply, so `convert.rs` generation refuses the
pair. `diff` reports one `conversion_blockers` note per such field, as `WF3004` in diagnostics:

```text
note: conversion generation refuses `types/task.required`: a required record field was added without a default; use option<T>, list<T>, or a new member
```

```json
"conversion_blockers": [
  {
    "kind": "required-addition",
    "path": "types/task.required"
  }
]
```

A retyped record field, variant case, or named type also blocks conversion generation: the
generated adapter has no conversion between two different values. `diff` reports it with
`"kind": "retyped"` and `WF3004`:

```text
note: conversion generation refuses `types/task.id`: the type changed; add a new member or type instead
```

```json
"conversion_blockers": [
  {
    "kind": "retyped",
    "path": "types/task.id"
  }
]
```

Alias spellings of one resolved type are not a change and not a blocker: `type id = u32` and
`u32` are one wire type and one Rust type. A changed alias target is a real retype and remains a
blocker. Parameters and function results have no generated conversion, so they are not blocker
entries.

Defaultable additions (`option<T>`, `list<T>`, or a named alias or structural type of one) do not
block generation and have no blocker note. The blocker notes are informational too: an addition
or retype is still a source/API-breaking change, and `--check` exits `1` for it.

Across a major-version boundary (`old.major != new.major`), `convert.rs` omits non-convertible
types per direction instead of refusing generation (`emitters.md` section 9.4). `diff` therefore
leaves `conversion_blockers` empty and populates `conversion_omissions` with the exact same
per-direction omission set that `convert.rs` writes at the top of the file (types with no namesake,
retypes, kind changes, required additions in old-to-new, variant case payload-presence changes,
result shape changes, changed foreign dependencies, and transitive omissions of containing types).
Human output prints one note per omission, `--json` emits `conversion_omissions`, and
`--format json|sarif` emits each omission as a `WF3004` note:

```text
note: conversion 0.2.0 -> 1.0.0 for `types/order` is omitted: field `total` is required in 1.0.0 and has no default
```

```json
"conversion_omissions": [
  {
    "from_version": "0.2.0",
    "to_version": "1.0.0",
    "type_path": "types/order",
    "reason": "field `total` is required in 1.0.0 and has no default"
  }
]
```

`diff --check` across a major boundary still exits `1` for a breaking change; a consumer that
expects a breaking major change omits `--check` on that comparison.

## Limits

`diff` compares root-contract structure and root stability attributes (`@since`, `@unstable`,
`@deprecated`), not root docs or generated file bytes. It does validate exact dependency-graph
identity, including dependency docs and stability. Generated output changes caused by an existing
contract (for example a new emitter convention) are detected by each emitter's `--check`, not by
`diff`.

`diff` reports guaranteed new-to-old conversion failures as notes, but it does not run `rustc`
or inspect generated file bytes. It does not report value-dependent failures such as a removed
variant or enum case, whose converter fails only for values that use that case. A future
memberless-inventory or semantic-diff rule would extend this document rather than inferring from
generated files.
