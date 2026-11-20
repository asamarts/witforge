# Contract diff and breaking-change reporting

Status: implemented 2026-11-05 (`W8-001`); endpoint ordering, missing-lock, and root/direct
declaration-order boundaries 2026-11-18 (`W10-026`); conversion-impact notes 2026-11-17
(`W11-036`); conversion blockers and API hardening 2026-11-18 (`W10-035`, `W10-036`); kind
changes and reused reserved numbers are wire-breaking 2026-11-19 (`W10-045`, `W10-048`); alias
spellings and retyped conversion blockers 2026-11-20 (`W10-057`); released-manifest and
dependency-alias endpoints 2026-11-20 (`W10-056`). The 2026-11-18 items are not in 0.5.0:
reversed-endpoint rejection, missing-lock guidance, and `conversion_blockers` are on `main` and
unreleased.

`witforge diff <old> <new>` compares two contract versions and reports what changed in three
independent compatibility views:

- **wire identity** — the append-only lockfile's numbers, kinds, and canonical member types;
- **source** — the WIT/component type shape;
- **generated API** — the Rust types, traits, and functions the Rust emitter generates.

A change can be compatible in one view and breaking in another. For example, adding an
`option<T>` record field is wire-compatible (the old data can be decoded and the generated
conversion defaults the field), but it changes the component type and breaks struct literals in
the generated Rust API.

Root-package docs and stability attributes are not compared: they are not wire identity and do
not change generated code. Dependency docs and stability are part of the exact pinned dependency
graph described below.

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

Both endpoints must resolve the identical pinned dependency graph: package identities and
versions, interface/type/function inventories and order, shapes and signatures, stability, and
docs. `diff` refuses the first mismatch as a contract violation instead of describing it as a
compatible local change. This is the same comparison every generator uses because dependency
bindings are one shared output across versions.

## Change categories

| Category | Meaning |
|---|---|
| `added` | A declaration exists only in the new version. |
| `added-defaulted` | An added record field has a type the generated old-to-new conversion can default: `option<T>`, `list<T>`, or an alias or named definition of one. |
| `removed` | A declaration exists only in the old version. |
| `reordered` | Common members changed declaration order. The report lists the old and new relative order of the common members. |
| `retyped` | A member, alias shape, or function result kept its name but changed type. |
| `kind-changed` | A named type kept its name but changed kind, such as record to enum. |
| `renumbered` | A common member kept its kind and type but changed wire number, including a function's method id. This is reported separately from a retype. |

Declaration order is compared for record fields, variant cases, enum cases, flags, functions,
function parameters, and world imports and exports. Additions and removals are ignored when
comparing the relative order of common members, as in the lockfile's order fingerprints.

## Compatibility rules

### Wire identity

Wire-breaking changes are:

- a changed wire number (`renumbered`), for a field, case, flag, parameter, or function method
  id;
- a new member whose number was reserved in the old lockfile, even though the member itself is
  an addition;
- a changed kind or canonical type of a numbered member (`retyped`);
- a type kind change (`kind-changed`), whether or not the old numbers stay reserved: every
  reference to the type keeps its canonical name while its encoding changes between an enum
  value, a bool field, and a message;
- removal of a member or a whole type, interface, or function whose old lockfile entries are not
  retained with their numbers and reserved.

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

These rules deliberately do not run `rustc` or compare generated files. They describe the
contract-level API surface, and `--check` is a gate, not a semantic-versioning replacement.

## Output

The human report has one line per change:

```text
retyped           record field       types/task.id                    number=1   wire:BREAKING source:BREAKING api:BREAKING  u8 -> u64
added-defaulted   record field       types/task.note                  number=3   wire:compatible source:BREAKING api:BREAKING
summary: 2 change(s), 2 breaking; wire identity: 1, source: 2, generated API: 2
result: BREAKING
```

`--json` emits the same information as JSON:

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
      "api_breaking": true
    }
  ],
  "summary": {
    "changes": 1,
    "breaking": 1,
    "wire_breaking": 1,
    "source_breaking": 1,
    "api_breaking": 1
  },
  "conversion_impacts": [],
  "conversion_blockers": []
}
```

`added` changes also carry `defaulted`; `reordered` changes carry `before` and `after`;
`renumbered` changes carry numeric `from` and `to`. A change's `number` is the new member's wire
number when the change concerns a numbered member, or the old number for a removal.

`--check` exits `0` when no change breaks any view and `1` when at least one does. A plain
`diff` exits `0` after reporting even a breaking change. Input, I/O, and parse errors use the
normal `witforge` exit conventions: `1` for contract/lockfile violations and `2` for usage,
I/O, and parse failures.

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

## Limits

`diff` compares root-contract structure, not root docs, root stability attributes, or generated
file bytes. It does validate exact dependency-graph identity, including dependency docs and
stability. Generated output changes caused by an existing contract (for example a new emitter
convention) are detected by each emitter's `--check`, not by `diff`.

`diff` reports guaranteed new-to-old conversion failures as notes, but it does not run `rustc`
or inspect generated file bytes. It does not report value-dependent failures such as a removed
variant or enum case, whose converter fails only for values that use that case. A future
memberless-inventory or semantic-diff rule would extend this document rather than inferring from
generated files.
