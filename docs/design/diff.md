# Contract diff and breaking-change reporting

Status: implemented 2026-11-05 (`W8-001`).

`witforge diff <old> <new>` compares two contract versions and reports what changed in three
independent compatibility views:

- **wire identity** — the append-only lockfile's numbers, kinds, and canonical member types;
- **source** — the WIT/component type shape;
- **generated API** — the Rust types, traits, and functions the Rust emitter generates.

A change can be compatible in one view and breaking in another. For example, adding an
`option<T>` record field is wire-compatible (the old data can be decoded and the generated
conversion defaults the field), but it changes the component type and breaks struct literals in
the generated Rust API.

Docs and stability attributes are not compared: they are not wire identity and do not change
generated code.

## Selecting the compared versions

Each argument is one of:

- a version directory containing `wire.lock`, compared as that exact version; or
- a WIT root containing `v<major>.<minor>/` directories, in which case the highest version under
  the root is compared.

Both endpoints must declare the same `namespace:name`. The selected version directories must
have valid WIT, a current lockfile, and a wire number for every live member; otherwise the
command fails rather than guessing.

## Change categories

| Category | Meaning |
|---|---|
| `added` | A declaration exists only in the new version. |
| `added-defaulted` | An added record field has a type the generated old-to-new conversion can default: `option<T>`, `list<T>`, or an alias or named definition of one. |
| `removed` | A declaration exists only in the old version. |
| `reordered` | Common members changed declaration order. The report lists the old and new relative order of the common members. |
| `retyped` | A member, alias shape, or function result kept its name but changed type. |
| `kind-changed` | A named type kept its name but changed kind, such as record to enum. |
| `renumbered` | A common member kept its kind and type but changed wire number. This is reported separately from a retype. |

Declaration order is compared for record fields, variant cases, enum cases, flags, functions,
function parameters, and world imports and exports. Additions and removals are ignored when
comparing the relative order of common members, as in the lockfile's order fingerprints.

## Compatibility rules

### Wire identity

Wire-breaking changes are:

- a changed wire number (`renumbered`);
- a changed kind or canonical type of a numbered member (`retyped`);
- a type kind change (`kind-changed`) whose old numbered entries are not preserved;
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
  }
}
```

`added` changes also carry `defaulted`; `reordered` changes carry `before` and `after`;
`renumbered` changes carry numeric `from` and `to`. A change's `number` is the new member's wire
number when the change concerns a numbered member, or the old number for a removal.

`--check` exits `0` when no change breaks any view and `1` when at least one does. A plain
`diff` exits `0` after reporting even a breaking change. Input, I/O, and parse errors use the
normal `witforge` exit conventions: `1` for contract/lockfile violations and `2` for usage,
I/O, and parse failures.

## Limits

`diff` compares contract structure, not docs, stability attributes, or generated file bytes.
Generated output changes caused by an existing contract (for example a new emitter convention)
are detected by each emitter's `--check`, not by `diff`. A future memberless-inventory or
semantic-diff rule would extend this document rather than inferring from generated files.
