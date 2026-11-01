# witforge wire lockfile (v2)

Status: specified 2026-10-25; implemented 2026-10-28. This document is normative for the
lockfile format and for the `check`/`lock` semantics. The decisions behind it are logged in
[`overview.md`](overview.md#9-decision-log).

The key words MUST, MUST NOT, SHOULD, and MAY are used as in RFC 2119.

## 1. Purpose

WIT declares record fields, variant cases, enum cases, flags, functions, and function
parameters positionally. Protobuf needs stable field numbers and method ids. The lockfile is
the committed, append-only map from each numbered member to a number that is unique within its
parent and never changes.

The lockfile guarantees wire identity, not full source identity. It does not record docs,
declaration order, worlds, or type definitions that have no numbered members. Released version
directories are frozen as a whole, which covers those cases.

## 2. Version directories and release status

- A contract version lives in `wit/v<major>.<minor>/` and contains a WIT package whose version
  is exactly `@<major>.<minor>.0`. Patch versions, pre-releases, and build metadata are refused
  by the subset checker. Both numbers in the directory name are ASCII digits without a sign or
  leading zeros; the generators refuse a name such as `v+0.1` or `v00.02` instead of skipping it.
- A version directory MUST NOT be a symlink. Every command (`check`, `lock`, and the generators),
  the released manifest, and `ci/check-release-freeze.sh` refuse one with exit `2`: a released
  version reached through a link could change through the link's target, which neither the
  manifest beside the link nor git (which records only the link's target string) would see. The
  WIT root and its parents may be symlinks.
- Each version directory has its own `wire.lock`.
- Rust module: `v<major>_<minor>`.
- Protobuf package/file: `v<major>` and `v<major>.proto`.
- `wit/released` is the explicit released-version manifest. It is plain text, one directory
  name per line (`v0.1`); leading and trailing ASCII whitespace (space, tab, CR, VT, FF) is
  trimmed, and blank lines and `#` comments are ignored. Entries are plain directory names
  resolved relative to the manifest's directory: an ASCII letter or digit followed by ASCII
  letters, digits, `.`, `_`, or `-` (`^[A-Za-z0-9][A-Za-z0-9._-]*$`), so `.`, `..`, hidden names,
  whitespace, separators, non-ASCII names, and absolute paths are rejected. The manifest is
  append-only: CI fails if an entry is removed, duplicated, or names a directory that does not
  exist or has no `wire.lock`. The tool and `ci/check-release-freeze.sh` apply the same entry
  rule; the tool rejects a malformed manifest, duplicate entries, and missing, symlinked, or
  lockfile-less listed directories.
- Commands accept `--released <path>`; the default is `<wit-dir>/../released` for a version
  directory (the parent of the canonicalised directory, so `.` and a symlinked WIT root find
  their real parent's manifest) and `<wit-root>/released` for generation. A missing default
  manifest means no version is treated as released. An explicitly supplied missing manifest is an
  error. Every command (`check`, `lock`, `rust`, `proto`, and `glue`) validates the manifest it
  uses and fails with exit `2` if it is missing when given explicitly or is malformed; only
  `lock` acts on its contents, and for `lock` an explicit manifest is read in addition to the
  defaults, never instead of them.
- A version listed in `wit/released` is frozen: its WIT, `wire.lock`, and its own per-version
  generated Rust module are immutable. Shared generated files (`mod.rs`, the per-major
  `v<major>.proto`, and the per-major glue module) may change when a new minor is added; their
  released members are protected by the lockfile and `--check`. In the proto schema the wire
  numbers and reserved names are append-only: a member removed in a later minor stays as a
  `reserved` number and name, while a type or function removed as a whole drops its messages
  from the schema and keeps its method number reserved.
- `ci/check-release-freeze.sh [WIT_ROOT]` (the root resolved from the current directory) reads
  the full history of the manifest, merges included: every entry a committed manifest listed
  must still be listed, and each listed directory must not be or hold a symlink, must hold a
  `wire.lock` that git tracks under exactly the entry's spelling (a case-insensitive file system
  would otherwise match `V0.1` to `v0.1` and check nothing), must have no untracked files
  (ignored ones included), and must match every commit that listed it, so a change that landed
  in a push whose CI run was cancelled, or came in through a merge, is still caught. When the
  WIT root was moved, the manifest's earlier paths (found by git's rename detection) are checked
  too, against what each commit held at the earlier path; a move that also rewrites most of the
  manifest is not detected as one, so a WIT root is moved in a commit that changes nothing else.
  A commit that lists a directory therefore fixes its content for as long as the commit stays in
  the history the check reads; rewriting history to drop it is outside the check, so the branch
  is protected. witforge's CI runs it on its fixture; a consumer can run it on its own WIT
  root.
- `lock` refuses to modify a released version directory, and refuses a `--lock` path inside one.
  The refusal compares resolved paths, not names: the version directory, the lockfile's
  directory, and every listed directory are canonicalised, so `.` and `..` components and a
  symlinked WIT root cannot bypass it. The manifest beside the version directory and the one
  beside the lockfile's directory, each at its written and its resolved location, always apply,
  whatever `--released` names, so neither a lockfile in another WIT root nor an explicit manifest
  that omits the directory gets past the manifest that lists it. `check`, `rust`, `proto`, and
  `glue` are allowed; they verify or reproduce the frozen contract. For a released version,
  `check` advises restoring the released WIT and lockfile rather than naming a `lock` command
  that `lock` refuses.
- A new major version starts a fresh lockfile. A new minor version starts as a copy of the
  previous minor's `wire.lock` and is then developed with `lock --against`.
- The generators (`rust`, `proto`, and `glue`) enforce that continuity across a WIT root: within
  each major, every minor's `wire.lock` must keep every entry of the previous minor's lockfile
  with the same number, kind, and canonical type, and must keep reserved entries reserved (rule
  1 of `check --against`, section 8). A lockfile that does not continue its predecessor fails
  generation with exit `1`, so a new minor cannot silently renumber the shared per-major proto
  schema. `check` and `lock` take a single version directory, so their continuity baseline is
  the `--against` lockfile.
- Docs-only patch versions are out of scope for 0.1. A docs-only patch requires a new version
  directory (later) or a semantic-diff relaxation (pre-golive consideration).

For ad-hoc directories such as test fixtures, the path need not be a version directory; output
names and the proto version are derived from the package version. The `v<major>.<minor>`
directory convention is enforced for a versioned `wit/` tree by the released manifest and CI,
not by every `check` invocation.

## 3. Keys

The package version is deliberately excluded from every key, so bumping the package version
never renumbers anything.

| Member | Key | Parent (numbers unique within) | Entry kind |
|---|---|---|---|
| record field | `namespace:package/interface/type.member` | the type | `field` |
| variant case | `namespace:package/interface/type.member` | the type | `variant-case` |
| enum case | `namespace:package/interface/type.member` | the type | `enum-case` |
| flag | `namespace:package/interface/type.member` | the type | `flag` |
| function (method id) | `namespace:package/interface.function` | the interface | `method` |
| function parameter | `namespace:package/interface#function.parameter` | the function | `param` |

The key grammar is strict:

```text
package          = namespace ":" name
type-member-key  = package "/" interface "/" type "." member
method-key       = package "/" interface "." function
param-key        = package "/" interface "#" function "." parameter
```

- `namespace`, `name`, `interface`, `type`, `member`, `function`, and `parameter` are WIT
  identifiers: kebab-case, no dots, slashes, hashes, equals signs, at signs, or whitespace.
- Names are the raw WIT names after `%` escaping is removed (`%type` is `type`).
- A `use` alias does not create a key. Members are locked where they are defined.
- Types and functions must be defined in the package's named interfaces; the subset checker
  refuses world-defined types/functions and foreign types/interfaces.
- Keys whose package prefix does not match the contract package are refused by `check`.
- Unknown key shapes, duplicate keys, and keys with an empty component are parse errors.

## 4. Entry format

The file starts with exactly this header line:

```text
# witforge wire lockfile v2
```

The v2 format is implemented. The reader rejects v1 files rather than migrating them. Do not
hand-edit the file; `witforge lock` renders the canonical form.

Every other non-comment line has the form:

```text
key = number kind [type] [reserved]
```

- One or more spaces or tabs separate the fields; the canonical render uses single spaces.
- `number` is a decimal integer without leading zeros.
- `kind` is one of `field`, `variant-case`, `enum-case`, `flag`, `method`, `param`.
- `type` is a canonical type token (section 5) with no whitespace.
- `reserved` is the literal final token and means the member was removed but its number and
  name stay taken.
- Reserved entries keep their kind and type. This preserves append-only kind/type checks and
  makes the history auditable.
- Blank lines and lines whose first non-whitespace character is `#` are ignored.
- Render writes the header, then entries sorted lexicographically by key, with LF line endings
  and a final newline. Comments are not preserved by render.
- The reader accepts the looser forms above, but `check` requires the canonical rendering byte
  for byte: a lockfile with comments, blank lines, tabs or extra spaces, CR line endings, no final
  newline, or another order fails `check` with exit `1` and "run `witforge lock`", which rewrites
  it. Only the canonical form is reviewed and frozen, so a tool that reformats the file cannot
  slip a change past review.
- A missing header, an unknown header version, malformed lines, unknown kind tokens, invalid
  numbers, duplicate keys, or two entries of one parent sharing a number are parse errors.

The kind/type matrix is:

| Kind | Type |
|---|---|
| `field` | required |
| `variant-case` | optional; absent means a payload-less case |
| `enum-case` | forbidden |
| `flag` | forbidden |
| `method` | optional; absent means the function has no result |
| `param` | required |

A `method` entry's type is the function's result type. Request/response messages are generated
from parameters and the result; their field numbers are not locked (the response result field is
always `1`).

Examples:

```text
example:plugin/types/span.start = 1 field u64
example:plugin/types/span.end = 2 field u64
example:plugin/types/applicability.safe = 1 enum-case
example:plugin/types/caps.git = 1 flag
example:plugin/types/edit.replace = 1 variant-case types/span
example:plugin/types/edit.delete-file = 2 variant-case
example:plugin/analyzer.analyze = 1 method result<list<types/finding>,string>
example:plugin/analyzer#analyze.path = 1 param string
example:plugin/analyzer#analyze.bytes = 2 param list<u8>
example:plugin/types/span.old = 3 field u32 reserved
```

## 5. Canonical types

The `type` token is deterministic and contains no whitespace. The model specified in
[`model.md`](model.md) is the source of the canonical rendering; the lockfile compares those
strings and does not reimplement type formatting.

```text
type          = primitive | named | list | option | result
primitive     = "bool" | "u8" | "u16" | "u32" | "u64"
              | "s8" | "s16" | "s32" | "s64" | "f32" | "f64" | "string"
named         = interface "/" type
list          = "list<" type ">"
option        = "option<" type ">"
result        = "result<" slot "," slot ">"
slot          = type | "_"
```

Canonicalization rules:

- Primitive types use their WIT spelling.
- A named record, variant, enum, or flags type is written as its defining interface and type:
  `types/span`. The defining interface is used even when the reference site uses
  `use ... as ...`.
- `type a = b` aliases are expanded recursively.
- Named `list`, `option`, and `result` definitions are expanded structurally. For example,
  `type tags = list<string>` is written as `list<string>` wherever it is used, not as `tags`.
  The named Rust alias is still emitted; the lockfile records the structure.
- `result` always has both slots. `result<T, _>`, `result<_, E>`, and bare `result` are written
  as `result<T,_>`, `result<_,E>`, and `result<_,_>`. There is no space after the comma.
- `list<u8>` stays `list<u8>` in the lockfile; the protobuf `bytes` projection is an emitter
  concern, not a wire-identity concern.
- Named list/option/result definitions have no locked members of their own; expanding them at
  use sites is what makes an inner type change visible.
- A named type is always qualified (`types/reserved`, `types/field`), so the bare token
  `reserved` can only be the reserved flag. A bare `reserved` in the type position is a parse
  error.
- `char`, tuples, maps, fixed-length lists, resources, handles, `future`, `stream`, and
  `error-context` are refused by the subset checker. The canonicalizer treats them as internal
  errors if they ever reach it.
- No spaces, comments, or escapes appear inside a type token.

## 6. Number allocation

- Numbers are unique within their parent: a type, an interface for method ids, or a function
  for parameters.
- A new parent starts at `1`.
- A new member gets `max(existing numbers in its parent) + 1`. Reserved numbers count, so they
  are never reused.
- If the next number falls in protobuf's reserved field range `19000..=19999`, skip to
  `20000`.
- If the next number would exceed `536870911`, `lock` fails with an error and writes nothing.
- Numbers never change while an entry exists. A dropped draft entry may free its number for a
  later new member; a released reserved entry never does.
- New members in one parent are numbered in deterministic model order: source order within the
  type/function. The rendered file is sorted by key, independently of assignment order.
- Reconcile is idempotent: a second run on the same source and lockfile changes nothing.

## 7. Modes and draft classification

`check` and `lock` have three modes:

| Mode | Baseline | Classification |
|---|---|---|
| default | the current lockfile | every entry is released |
| `--against <previous wire.lock>` | the previous lockfile | entries in the previous file are released; current entries not in it are drafts |
| `--first` | none | every entry is a draft |

- `--against` and `--first` are mutually exclusive.
- `--first` is only for a version with no previous lockfile baseline: the first version of a
  package or the first version of a new major. It must not be used to rewrite an existing
  released or developed version.
- `--against` is a development command. It is used for the version being prepared, with the
  previous minor's committed lockfile as the baseline. A new major starts a fresh lockfile and
  uses `--first`, not `--against` from the previous major.
- The current version's `wire.lock` must exist for `--against`; the workflow starts a new minor
  by copying the previous lockfile. A missing current lockfile is an error, not an implicit
  copy, and the error says to copy the previous lockfile first. The previous lockfile must parse
  and must belong to the same package namespace and name; keys from another package are an
  error.
- The current version's `wire.lock` must also exist in default mode. A missing lockfile is an
  error (exit `2`) that names both ways forward: `--first` for a version with no baseline, or a
  copy of the previous minor's `wire.lock` developed with `--against` for a new minor. Only
  `--first` treats a missing lockfile as empty. A default-mode lock that started from an empty
  lockfile would renumber every member of a new minor.
- A released version directory cannot be locked. The released manifest is the tool's source of
  truth for this.
- `@unstable` and `@since` are metadata. Every item in the version directory is part of that
  version, including unstable items. Removing an unstable item after release requires a new
  version.

## 8. `check` semantics

`check` never writes. It loads the package, runs the subset checker, parses the lockfile, and
reports every discrepancy. If the contract has subset violations, `check` reports only those and
does not compare the lockfile: a contract outside the subset has no model, so every locked member
would otherwise look removed. Every entry, and every entry of an `--against` baseline, must
belong to the contract's package, also when the contract has no members. Exit code `1` means
contract or lockfile violations, a lockfile that is not in canonical form included; exit code
`2` means usage, IO, parse, or internal errors.

Without a baseline (default mode):

1. Every live member has a non-reserved entry of the same kind and canonical type. A missing or
   mismatched entry is reported as "run `witforge lock`".
2. A live member whose entry is reserved is a reintroduced member and is an error.
3. An entry whose member is no longer live must be `reserved`; if it is not reserved, report
   "run `witforge lock` to reserve it".
4. Every reserved entry that is still live is a reintroduced member and is an error.
5. A type/kind change is an error. Without a baseline the tool cannot know whether the member
   is a draft, so it is treated as released; use `--against` or `--first` while developing.

With `--against <previous>`:

1. Every entry in the previous lockfile must exist in the current lockfile with the same
   number, kind, and canonical type, and must not be un-reserved. A missing, renumbered,
   retyped, re-kinded, or un-reserved released entry is an error.
2. A previous entry that is reserved must stay reserved. If the current source reintroduces it,
   it is an error.
3. A current entry not present in the previous lockfile is a draft:
   - if its member is live and its kind/type matches, it is fine;
   - if its member is live but its kind/type differs, report "run `witforge lock` to update the
     draft";
   - if its member is not live, report "run `witforge lock` to drop the stale draft".
4. A live member with a released reserved entry is a reintroduced released member and is an
   error.
5. A live member with a stale draft reserved entry is a draft that was removed and re-added;
   report "run `witforge lock`" (the stale entry is dropped and a new number is assigned).
6. All live members must have entries, and released removed members must be reserved.

With `--first`:

- There is no append-only baseline.
- Every existing entry is a draft.
- Live members must have entries of the right kind/type; otherwise report "run `witforge lock`".
- Entries for non-live members, including reserved entries, are stale drafts; report
  "run `witforge lock`".
- A live member with a reserved draft entry is a stale draft; report "run `witforge lock`";
  it is not a reintroduction error because the name was never released.

## 9. `lock` semantics

`lock` loads the package and runs the subset checker. It writes nothing if there are subset
violations, parse errors, reintroduced released members, released type/kind changes, renumbered
or un-reserved released entries, exhausted number space, or a released-directory refusal.

It then applies the mode:

- **Default:** treat every entry as released. Add new members, reserve removed members, and
  fail on kind/type changes, renumbering, un-reserving, and reintroductions.
- **`--against`:** released entries follow the default rules. Draft entries may be retyped,
  re-kinded, or dropped. New members are added. A draft that was removed and re-added can get a
  new number after the stale entry is dropped.
- **`--first`:** every entry is a draft. Live drafts may be retyped or re-kinded; removed
  entries are dropped; new members are added; there is no append-only baseline.

The update order is:

1. Validate the previous baseline and the released entries.
2. Drop stale draft entries: entries not present in the previous lockfile (or, in `--first`
   mode, every entry) that are either not live or reserved.
3. Retype/re-kind live draft entries.
4. Reserve removed released entries.
5. Add new live members with the next number in their parent.
6. Fail without writing if any released entry would change or any reintroduction is detected.
7. Render and write atomically (temporary file in the same directory, then rename).

`lock` prints one line per change (`added`, `reserved`, `updated`, `dropped`) and exits `0`.
It uses the same exit codes as `check`: when it writes nothing because of a contract or lockfile
violation (subset violations, reintroduced members, a changed, renumbered, missing, or
un-reserved released entry, a mismatched `--against` baseline, or an exhausted number space) it
exits `1`; it refuses a released directory or a lockfile path inside one, and reports usage, IO,
parse, and internal errors, with exit `2`. A no-op lock still writes the canonical rendering
(header, sorted entries, LF) unless the file is already byte-identical, in which case the file is
left alone.

## 10. Reserved members and reintroduction

- A removed released member becomes `key = N kind [type] reserved`.
- Its number and name stay reserved forever. The protobuf emitter emits both the number and the
  derived proto name.
- A member that comes back under a **released** reserved name is refused. `check` reports an
  error and `lock` writes nothing. A stale draft reserved entry is instead dropped and the live
  member is re-added.
- A removed draft drops its entry; no `reserved` gap is left. If the name returns, it is a new
  draft with a possibly new number.
- The lockfile is never compacted, and released entries are never deleted.
- Renames are removals plus additions. There are no rename markers. A released member is
  replaced by adding a new member, deprecating the old one in docs, and removing it in a new
  major version.

## 11. Function and parameter locking

- A method-id entry is `namespace:package/interface.function`, kind `method`; its number is
  unique within the interface and starts at `1`.
- Its type is the function's result canonical type, or absent if the function has no result.
- A parameter entry is `namespace:package/interface#function.parameter`, kind `param`; its
  number is unique within that function and starts at `1`. Its type is required.
- Removing a released function reserves its method id and every parameter entry; a draft
  function's entries are dropped instead.
- Renaming a function reserves the old method id and parameters and adds the new ones. For a
  draft, the old entries are dropped instead.
- Removing a released parameter reserves it; a parameter that comes back under the same name is
  reintroduced and refused. A draft parameter is dropped instead.
- A function's response message is generated with a single `result = 1` field. That field is
  not locked; a change to the result type is caught by the method entry's canonical type.
- Method-id enum values and request field numbers are locked; the zero `UNSPECIFIED` enum value
  is not.
- Function and parameter declaration order is not tracked in the MVP (see the pre-golive task
  in [`ROADMAP.md`](../../ROADMAP.md)).

## 12. Type-member locking

- A record field entry has kind `field` and a required type.
- A variant case entry has kind `variant-case`; the type is present for a payload and absent for
  a payload-less case.
- An enum case entry has kind `enum-case` and no type.
- A flag entry has kind `flag` and no type.
- Removing a released member reserves it; reintroducing it is refused. A draft member's entry is
  dropped instead, and a returning draft gets a new entry.
- Changing a member's type or kind for a released version is an error. A draft may change.
- A member moved between types is a removal from the old parent plus an addition in the new one.
- A type removed from the source reserves all of its member entries. A type with no members
  (for example a named list/option/result) has no entries of its own; released-directory
  freezing and the canonical expansion at use sites cover it.
- Declaration order is not tracked in the MVP.

## 13. Determinism and file handling

- `wire.lock` is UTF-8, LF-only, and ends with a newline.
- The header is the first line. Entries are rendered sorted by key.
- Numbers have no leading zeros; types have no whitespace.
- Render is deterministic across platforms and runs; it contains no timestamps, tool versions,
  or absolute paths.
- Parse errors name the line number and the offending text.
- `lock` writes atomically and only on success.
- A lockfile with no entries is valid and renders as the header alone.

## 14. Testing requirements

The implementation must cover:

- parse/render round trips and every invalid form in section 4;
- the kind/type matrix;
- canonical type expansion for aliases, named list/option/result, `use ... as ...`, and
  one-sided/bare results;
- number allocation, the `19000..=19999` skip, the maximum number error, and per-parent scopes;
- idempotent reconcile and no renumbering;
- reserved and reintroduced members;
- draft rename/removal/retype, including a draft removed and re-added;
- released type/kind/renumber/un-reserve errors;
- `--first`, `--against`, default mode, and released-directory refusal, including through `.`,
  `..` components, a `--lock` path inside a released directory from another WIT root or with an
  explicit manifest that omits it, and a symlinked version directory refused by every command;
- keys from another package refused in every mode, also for a contract with no members, and
  parameter keys whose interface is not an identifier refused when parsed;
- `check` refusing a lockfile that is not in canonical form (section 4);
- an unchanged lockfile left untouched;
- a missing lockfile refused in default mode and with `--against`, and accepted with `--first`;
- the `wit/released` manifest: append-only, duplicate/missing entries, the plain-name entry rule,
  and a released directory refusing `lock`;
- a two-version fixture with a copied lockfile and an added/removed/renamed member, and a new
  minor whose lockfile does not continue the previous one failing every generator;
- CLI integration tests for `check`, `lock`, `--first`, `--against`, `--released` (missing and
  malformed manifests for every command), exit codes, and no-write-on-error;
- property tests for idempotence, append-only behavior, and random add/remove/retype sequences.

## 15. Deferred and pre-golive

- Declaration-order fingerprinting (the order-tracking Option C) is a pre-golive task.
- A type/interface/world inventory for memberless definitions is covered by released-directory
  freezing for now; reconsider before 1.0.
- Docs-only patch versions.
- A configuration file for multiple emitters/version roots.
- Replacing the lockfile with in-source WIT annotations if component-model issue 695 lands with a
  suitable annotation.
