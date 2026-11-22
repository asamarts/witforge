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
worlds, or type definitions that have no numbered members. An additive section records
declaration-order fingerprints for numbered type members, functions, and function parameters
(section 16), without adding an order field to each entry. Released version directories are
frozen as a whole, which covers the remaining source-level cases.

## 2. Version directories and release status

- A contract version lives in `wit/v<major>.<minor>/` and contains a WIT package whose version
  is exactly `@<major>.<minor>.0`. Patch versions, pre-releases, and build metadata are refused
  by the subset checker. Every command refuses a directory so named whose package declares
  another version (`wit/v0.1` holding `@0.2.0`) with exit `1`: `check` and `lock` read the name
  of the directory they are given, resolved (so `.` counts), and the generators that of every
  version directory. Both numbers in the directory name are ASCII digits without a sign or
  leading zeros; every command refuses a name such as `v+0.1` or `v00.02` (exit `2`) instead of
  reading it as a version or skipping it.
- A version directory MUST NOT be a symlink. Every command (`check`, `lock`, and the generators),
  the released manifest, and `ci/check-release-freeze.sh` refuse one with exit `2`: a released
  version reached through a link could change through the link's target, which neither the
  manifest beside the link nor git (which records only the link's target string) would see. The
  WIT root and its parents may be symlinks. A command sees only the path it is given: run inside
  a symlinked version directory (`cd wit/v0.3 && witforge lock .`), it sees the working directory
  the operating system resolved, the link's target, and cannot tell that it was reached through
  a link, so it reads (and `lock` writes) the target as an unlinked directory. The freeze check
  still refuses a released directory that is a symlink.
- A directory listed in `wit/released` MUST NOT contain a symlink in any file or subdirectory.
  The released-manifest loader and every command that reads it refuse one with exit `2`: a
  symlinked `contract.wit`, `wire.lock`, or `deps/` entry could change outside the manifest's
  view. The freeze script independently checks the tracked history and working tree. This rule
  was added in 0.6.0; releases through 0.5.0 refused only a listed directory that is itself a
  symlink.
- Each version directory has its own `wire.lock`.
- Rust module: `v<major>_<minor>`.
- Protobuf package/file: `v<major>` and `v<major>.proto`.
- `wit/released` is the explicit released-version manifest. It is plain text, one entry per line:
  either `<name>` (live) or `<name> retired` separated by ASCII spaces or tabs (`v0.1` or
  `v0.1 retired`). Leading and trailing ASCII whitespace (space, tab, CR, VT, FF) is trimmed, and
  blank lines and `#` comments are ignored. Directory names are plain directory names resolved
  relative to the manifest's directory: an ASCII letter or digit followed by ASCII letters,
  digits, `.`, `_`, or `-` (`^[A-Za-z0-9][A-Za-z0-9._-]*$`), so `.`, `..`, hidden names,
  whitespace inside names, separators, non-ASCII names, and absolute paths are rejected. Unknown
  trailing tokens are rejected as malformed. The `retired` syntax is first read by **0.7.0** (not
  yet cut); earlier releases reject `<name> retired` as malformed. The manifest is append-only: CI
  fails if an entry is removed, duplicated, un-retired (`retired` back to live), or names a
  directory that does not exist or has no `wire.lock`. The tool and `ci/check-release-freeze.sh`
  apply the same entry rule; the tool rejects a malformed manifest, duplicate entries, and missing,
  symlinked, or lockfile-less listed directories.
- **Version retirement:** marking a released version `retired` records that its directory stays on
  disk and stays frozen (still validated for lockfile continuity into any later minor of its major,
  still refused by `lock`, and still frozen by `witforge freeze`), while every generator stops
  emitting it (so stale-file removal deletes its plain Rust module, JSON Schema file, and adapters
  from the consumer tree). Only a contiguous oldest prefix of versions in a WIT root may be
  retired, and at least one live version must remain; retiring a non-oldest version (such as
  `v0.2` while `v0.1` is still live) or retiring every version is refused (`WF1100`). Because a
  major's `.proto` file and glue module are built from its highest minor and retain all
  reservations, retiring an older minor of a live major does not change the wire schema; retiring
  every version of a major stops emitting that major's `.proto` file and glue module (and any
  dependency bindings used only by that major). `witforge freeze` accepts the one-way transition
  of a committed entry from live to `retired`, and refuses every other manifest change on prior
  entries (including `retired` back to live, removal, or edits inside a retired directory).
- Commands accept `--released <path>`; the default is `<wit-dir>/../released` for a version
  directory (the parent of the canonicalised directory, so `.` and a symlinked WIT root find
  their real parent's manifest) and `<wit-root>/released` for generation. A missing default
  manifest means no version is treated as released. An explicitly supplied missing manifest is an
  error. Every command that takes `--released` (`check`, `lock`, `rust`, `proto`, `schema`, and
  `glue`) validates the manifest it uses and fails with exit `2` if it is missing when given
  explicitly or is malformed. `lock` refuses to edit any version listed in the explicit manifest
  in addition to the default manifests; whole-root `check` and every generator (`rust`, `proto`,
  `schema`, `glue`, and `generate`) also enforce retirement order and exclude entries marked
  `retired` in the manifest they use. `diff` does not take `--released` and does not read the
  manifest.
- A version listed in `wit/released` is frozen: its WIT, `wire.lock`, and its own per-version
  generated Rust module are immutable. Shared generated files (`mod.rs`, the per-major
  `v<major>.proto`, and the per-major glue module) may change when a new minor is added; their
  released members are protected by the lockfile and `--check`. In the proto schema the wire
  numbers and reserved names are append-only: a member removed in a later minor stays as a
  `reserved` number and name, while a type or function removed as a whole drops its messages
  from the schema and keeps its method number reserved.
- `witforge freeze [<WIT_ROOT>] [--config PATH] [--project NAME] [--base REV]` (also invoked by
  the wrapper `ci/check-release-freeze.sh [WIT_ROOT]`) reads the full history of the manifest,
  merges included: every entry a committed manifest listed must still be listed, and each listed
  directory must not be or hold a symlink, must hold a `wire.lock` that git tracks under exactly
  the entry's spelling (a case-insensitive file system would otherwise match `V0.1` to `v0.1` and
  check nothing), must have no untracked files (ignored ones included), and must match every
  commit that listed it, so a change that landed in a push whose CI run was cancelled, or came in
  through a merge, is still caught. Every violation is collected and reported (exit `1`,
  `WF1109`) rather than stopping at the first. A shallow clone, missing `git`, or directory
  outside any Git repository fails with exit `2` (naming `fetch-depth: 0` for shallow clones).
  `--base <REV>` optionally bounds the history scan to `<REV>` and `<REV>..HEAD`. When `--config`
  is given without `--project`, every configured project's WIT root is verified. When the WIT
  root was moved, the manifest's earlier paths (found by git's rename detection) are checked too,
  against what each commit held at the earlier path. Only renames count: git reports a new
  manifest that resembles one that still exists, such as a second WIT root's, as a copy, and the
  history beyond a copy belongs to the other manifest. A move that also rewrites most of the
  manifest, or leaves a manifest at the earlier path, is not detected as one, so a WIT root is
  moved in a commit that changes nothing else. Paths are given to git literally and read back
  unquoted, so a WIT root whose path holds `*`, `?`, `[`, or non-ASCII characters is checked as
  it is spelled.
  A commit that lists a directory therefore fixes its content for as long as the commit stays in
  the history the check reads; rewriting history to drop it is outside the check, so the branch
  is protected. witforge's CI runs it on its fixture and on `examples/evolved/wit`; a consumer can
  run `witforge freeze` directly or through the GitHub Action's `freeze` input.
- `lock` refuses to modify a released version directory, and refuses a `--lock` path inside one.
  The refusal compares resolved paths, not names: the version directory, the lockfile's
  directory, and every listed directory are canonicalised, so `.` and `..` components and a
  symlinked WIT root cannot bypass it. The manifest beside the version directory, the one beside
  the lockfile's directory, and the one beside each of that directory's ancestors named like a
  version directory (`v<major>.<minor>`), each at its written and its resolved location, always
  apply, whatever `--released` names, so neither a lockfile in another WIT root, nor one in a
  subdirectory of a released directory (`--lock other/v0.1/sub/wire.lock`), nor an explicit
  manifest that omits the directory gets past the manifest that lists it. `check`, `rust`,
  `proto`, `schema`, `glue`, and `diff` are allowed; they verify, reproduce, or compare the
  frozen contract. For a released
  version, `check` advises restoring the released WIT and lockfile rather than naming a `lock`
  command that `lock` refuses.
- A new major version starts a fresh lockfile. A new minor version starts as a copy of the
  previous minor's `wire.lock` and is then developed with `lock --against`.
- The generators (`rust`, `proto`, `schema`, and `glue`) enforce that continuity across a WIT
  root: within
  each major, every minor's `wire.lock` must keep every entry of the previous minor's lockfile
  with the same number, kind, and canonical type, and must keep reserved entries reserved (rule
  1 of `check --against`, section 8). A lockfile that does not continue its predecessor fails
  generation with exit `1`, so a new minor cannot silently renumber the shared per-major proto
  schema. `check` and `lock` take a single version directory, so their continuity baseline is
  the `--against` lockfile.
- Docs-only patch versions are out of scope for 0.x and declined for 1.0. A directory named
  `v0.1.1` is not a version directory and is ignored by root discovery. A docs-only change uses a
  new minor, or is caught by the release freeze if it edits a released directory; an explicit
  patch shape would need a documented semantic-diff rule.

For ad-hoc directories such as test fixtures, the path need not be a version directory: a
directory whose name is not spelled `v<major>.<minor>` may hold any version, and output names
and the proto version are derived from the package version. A directory with such a name holds
that version for every command; the layout of a versioned `wit/` tree as a whole is enforced by
the generators, the released manifest, and CI.

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
  refuses world-defined types and functions. Named types from standard `deps/` packages are
  allowed and are canonicalized with package identity; worlds may import and export dependency
  interfaces, which do not create lock entries.
- Keys whose package prefix does not match the contract package are refused by `check`; a
  foreign type appears only inside a local entry's canonical type.
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

- One or more ASCII spaces or tabs separate the fields; the canonical render uses single spaces.
  U+00A0, other Unicode spaces, vertical tab, and form feed are not separators; they make the
  field that contains them invalid.
- `number` is a decimal integer without leading zeros.
- `kind` is one of `field`, `variant-case`, `enum-case`, `flag`, `method`, `param`.
- `type` is a canonical type token (section 5) with no whitespace.
- `reserved` is the literal final token and means the member was removed but its number and
  name stay taken.
- Reserved entries keep their kind and type. This preserves append-only kind/type checks and
  makes the history auditable.
- Blank lines and lines whose first non-space-or-tab character is `#` are ignored.
- A UTF-8 byte-order mark at the start of the file is refused as a missing header. Write the
  lockfile as UTF-8 without a BOM, as `lock` does.
- Render writes the header, then entries sorted lexicographically by key, with LF line endings
  and a final newline. Comments are not preserved by render.
- The reader, and so `lock` and the generators, accepts the looser forms above. `check`
  requires an unreleased version's lockfile in canonical form: the rendering byte for byte, once
  CRLF line endings are read as LF. A lockfile with comments, blank lines, tabs or extra spaces,
  a CR that does not end a line, no final newline, or another order fails `check` with exit `1`
  and "run `witforge lock`", which rewrites it in canonical form and so drops its comments. Only
  the canonical form is reviewed, so a tool that reformats the file cannot slip a change past
  review.
- A released version's lockfile need not be canonical: `lock` refuses to rewrite it, and
  `ci/check-release-freeze.sh` keeps it as it was released, so `check` accepts any form the
  reader accepts (a CRLF checkout, or a comment in a lockfile released with 0.1.0) and compares
  its entries with the contract as for any version.
- Git rewrites line endings at checkout under `core.autocrlf=true`, the Git for Windows default.
  A consumer pins its contracts to LF in `.gitattributes` (`wire.lock text eol=lf` and
  `*.wit text eol=lf`), so every checkout holds what `lock` wrote and what was reviewed.
- A missing header, an unknown header version, malformed lines, unknown kind tokens, a kind
  under the wrong key shape, invalid numbers, duplicate keys, or two entries of one parent
  sharing a number are parse errors, as is a canonical type token outside section 5.

The kind/type matrix is:

| Kind | Type |
|---|---|
| `field` | required |
| `variant-case` | optional; absent means a payload-less case |
| `enum-case` | forbidden |
| `flag` | forbidden |
| `method` | optional; absent means the function has no result |
| `param` | required |

The entry's kind must also match its key shape: a type key holds `field`, `variant-case`,
`enum-case`, or `flag`; an interface key holds `method`; and a `#` function key holds `param`.
Any other pairing is a parse error naming the line and the key shape.

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
              | namespace ":" package "/" interface "/" type
list          = "list<" type ">"
option        = "option<" type ">"
result        = "result<" slot "," slot ">"
slot          = type | "_"
```

Canonicalization rules:

- The token is parsed with this grammar, not checked only for a slash. A primitive is exactly
  its WIT spelling; `u8:foo/bar` is not a primitive with an annotation or a foreign name. A
  foreign name is exactly `namespace:package/interface/type`: too few or too many components,
  including a trailing slash, are parse errors.
- Primitive types use their WIT spelling.
- A named record, variant, enum, or flags type is written as its defining interface and type:
  `types/span`. The defining interface is used even when the reference site uses
  `use ... as ...`.
- A local `type a = b` alias is expanded recursively to its target. A named type imported from
  a `deps/` dependency is recorded by its package-qualified name without expansion, whether it
  is an alias, a named `list`/`option`/`result`, or a member-bearing type:
  `ex:shared/types/id`, `ex:shared/types/items`. A local `use ... as ...` alias of a foreign
  type is expanded to that foreign name. The version is not part of the name.
- Local named `list`, `option`, and `result` definitions are expanded structurally. For example,
  `type tags = list<string>` is written as `list<string>` wherever it is used, not as `tags`.
  The named Rust alias is still emitted; the lockfile records the structure. A named structural
  type defined in a dependency keeps its dependency-qualified name
  (`ex:shared/types/items`), not its structural expansion.
- `result` always has both slots. `result<T, _>`, `result<_, E>`, and bare `result` are written
  as `result<T,_>`, `result<_,E>`, and `result<_,_>`. There is no space after the comma.
- `list<u8>` stays `list<u8>` in the lockfile; the protobuf `bytes` projection is an emitter
  concern, not a wire-identity concern. A list spelled with an alias whose resolved element is
  `u8` canonicalizes to the same `list<u8>` token, so the lockfile cannot distinguish the two
  spellings and every emitter must project both the same way.
- Named list/option/result definitions have no locked members of their own; expanding them at
  use sites is what makes an inner type change visible. A foreign alias therefore does not
  expose its target's change through the canonical string; `check --against` compares the
  pinned dependency graph with the same rule the generators use (section 7), and the generators
  refuse a changed graph when the root is generated.
- A named type is always qualified (`types/reserved`, `types/field`), so the bare token
  `reserved` can only be the reserved flag. A bare `reserved` in the type position is a parse
  error.
- `char`, tuples, maps, fixed-length lists, resources, handles, `future`, `stream`, and
  `error-context` are refused by the subset checker. The canonicalizer treats them as internal
  errors if they ever reach it.
- No spaces, comments, or escapes appear inside a type token.
- A type token is at most 1024 bytes and nests `list`/`option`/`result` at most 32 levels deep.
  The reader refuses a longer or deeper token as a parse error (exit `2`) before recursive
  parsing, matching the model's expansion limits in [`model.md`](model.md#41-expansion-limits).
  A lockfile cannot hold an alias chain: aliases are already expanded in the canonical form.

## 6. Number allocation

- Numbers are unique within their parent: a type, an interface for method ids, or a function
  for parameters.
- One parent has one kind. A type parent holds only `field`, `variant-case`, `enum-case`, or
  `flag` entries, never a mix; an interface parent holds only `method` entries and a function
  parent only `param` entries. A named type has one kind, so a reconciliation that would leave
  the old kind reserved beside new members of another kind is refused with exit `1`, naming the
  parent and both kinds. `check` and every generator refuse a lockfile that already holds such
  a mix. A contract that needs the other kind uses a new type name in a new version; a draft
  type may change kind because `--first` drops its draft entries before adding the new ones.
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
- In a WIT root, `--first` is accepted only for the lowest minor of a major: `v0.1` beside
  `v0.2`, `v1.0` beside `v0.x`, and `v0.3` when the root starts there. It is refused in `v0.2`
  beside `v0.1` with exit `1`; the message names the predecessor and the `--against` command
  that continues it. Running `--first` again on the first minor while it is still a draft stays
  allowed. A version directory outside a WIT root, or one whose name is not `v<major>.<minor>`,
  has no siblings to judge by, and `--first` is accepted.
- `--against` is a development command. It is used for the version being prepared, with the
  previous minor's committed lockfile as the baseline. A new major starts a fresh lockfile and
  uses `--first`, not `--against` from the previous major. When the baseline is the `wire.lock`
  of a sibling `v<major>.<minor>` directory, an older baseline must be the immediate
  predecessor: a baseline from another major is refused with exit `1` and the `--first` remedy,
  and one that skips an existing intermediate minor is refused while naming the predecessor. A
  baseline file outside a version directory is accepted as given.
- When the baseline file belongs to a version directory, `check --against` also compares the
  current and baseline pinned dependency graphs with the generators' shared rule: package pins,
  versions, interface/type/function inventories and order, shapes, stability, and docs. A
  difference fails with exit `1` and the generators' message. A baseline copied outside a
  version directory has no WIT to compare; `check` says so in a note, and CI must run a generator
  over the WIT root to prove the graph.
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
contract or lockfile violations, an unreleased version's lockfile that is not in canonical form
(section 4) and a version directory holding another version (section 2) included; exit code `2`
means usage, IO, parse, or internal errors. Every mode also refuses a parent that mixes member
kinds (section 6); the error names the parent and both kinds.

A run collects every released discrepancy before it fails, in lockfile key order: released
kind/type changes, renumbering, un-reserving, missing entries, order changes, and a mixed-kind
parent. Reintroduced members are collected separately in `Changes.reintroduced` and reported in
the same run, so they never hide a released discrepancy. An entry whose kind and type both
changed is one diagnostic naming both. A mixed parent is one diagnostic for the parent, not one
per member. `lock` reports the same list and writes nothing when it contains any discrepancy,
however many of the others it could have reconciled.

Without a baseline (default mode):

1. Every live member has a non-reserved entry of the same kind and canonical type. A missing or
   mismatched entry is reported as "run `witforge lock`".
2. A live member whose entry is reserved is a reintroduced member and is an error.
3. An entry whose member is no longer live must be `reserved`; if it is not reserved, report
   "run `witforge lock` to reserve it".
4. Every reserved entry that is still live is a reintroduced member and is an error.
5. A type/kind change is an error. Without a baseline the tool cannot know whether the member
   is a draft, so it is treated as released; the reintroduction message points at `--against`
   while developing.

With `--against <previous>`:

1. Every entry in the previous lockfile must exist in the current lockfile with the same
   number, kind, and canonical type. A missing, renumbered, retyped, or re-kinded released
   entry is an error.
2. A previous entry that is reserved must stay reserved. If the current source reintroduces it,
   it is an error.
3. A previous **live** entry may be reserved in the current lockfile when its member was removed
   in the draft. If the current source restores that member with the previous number, kind, and
   type, the entry is un-reserved: this is not a reintroduction, and the resulting lockfile is
   the same as one in which the removal never happened. Restoring it with another kind or type
   is a released change and is refused. `check` reports the needed un-reservation with `WF1108`
   until `lock --against` writes it; `lock` reports `WF1205` for the restored entry.
4. A current entry not present in the previous lockfile is a draft:
   - if its member is live and its kind/type matches, it is fine;
   - if its member is live but its kind/type differs, report "run `witforge lock` to update the
     draft";
   - if its member is not live, report "run `witforge lock` to drop the stale draft".
5. A live member with a released reserved entry is a reintroduced released member and is an
   error.
6. A live member with a stale draft reserved entry that is not in the previous lockfile is a
   draft that was removed and re-added; report "run `witforge lock`" (the stale entry is
   dropped and a new number is assigned).
7. All live members must have entries, and released removed members must be reserved.

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
or un-reserved released entries, a parent that would mix member kinds (section 6), a misplaced
`--first` or `--against` baseline (section 7), exhausted number space, or a released-directory
refusal.

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
5. Un-reserve a draft removal when the previous entry is live and the current source restores
   it with the previous number, kind, and type.
6. Add new live members with the next number in their parent.
7. Fail without writing if any discrepancy (released entry, order, or mixed-kind parent) or any
   reintroduction is detected.
8. Render and write atomically (temporary file in the same directory, then rename).

`lock` prints one line per change (`added`, `reserved`, `updated`, `dropped`) and exits `0`. A
restored draft removal is simply un-reserved and has no change line.
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
- A released entry that the draft lock reserved because the source removed it may be restored
  by `lock --against` when the baseline has it live with the same number, kind, and type. The
  entry is un-reserved; this is not a reintroduction. Restoring a different kind or type is a
  released change and is refused. A reserved baseline entry stays reserved.
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
- Removing a released parameter reserves it; restoring a draft removal through `--against`
  follows section 8. A parameter reserved in the baseline cannot come back. A draft parameter
  is dropped instead.
- A function's response message is generated with a single `result = 1` field. That field is
  not locked; a change to the result type is caught by the method entry's canonical type.
- Method-id enum values and request field numbers are locked; the zero `UNSPECIFIED` enum value
  is not.
- Function declaration order within an interface and function parameter declaration order are
  recorded by the additive order section (section 16).

## 12. Type-member locking

- A record field entry has kind `field` and a required type.
- A variant case entry has kind `variant-case`; the type is present for a payload and absent for
  a payload-less case.
- An enum case entry has kind `enum-case` and no type.
- A flag entry has kind `flag` and no type.
- Removing a released member reserves it; reintroducing a member reserved in the baseline is
  refused. A draft removal can be restored through `--against` when the baseline member is live
  with the same number, kind, and type (section 8). A draft member's entry is dropped instead,
  and a returning draft gets a new entry.
- Changing a member's type or kind for a released version is an error. A draft may change.
- A released type that changes kind cannot orphan its reserved entries beside the new kind;
  section 6 refuses the mix. Give the new type a new name in a new version.
- A member moved between types is a removal from the old parent plus an addition in the new one.
- A type removed from the source reserves all of its member entries. A type with no members
  (for example a named list/option/result) has no entries of its own; released-directory
  freezing and the canonical expansion at use sites cover it.
- Type-member declaration order is recorded by the additive order section (section 16).

## 13. Determinism and file handling

- `wire.lock` is UTF-8, LF-only, and ends with a newline.
- The header is the first line. Entries are rendered sorted by key.
- When present, the order section follows the entries: the marker line, then one order line per
  live parent, sorted by parent key.
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
- a draft removal restored through `--against` for every member kind, with the baseline live
  entry's number, kind, and type, while a baseline reserved entry and a changed restore are
  still refused;
- draft rename/removal/retype, including a draft removed and re-added;
- released type/kind/renumber/un-reserve errors;
- `--first`, `--against`, default mode, and released-directory refusal, including through `.`,
  `..` components, a `--lock` path inside a released directory, or a subdirectory of one, from
  the same or another WIT root or with an explicit manifest that omits it, and a symlinked
  version directory refused by every command, also when no manifest lists it;
- a version directory holding another version, or with a misspelled version name, refused by
  `check` and `lock`;
- keys from another package refused in every mode, also for a contract with no members, and
  parameter keys whose interface is not an identifier refused when parsed;
- `check` refusing an unreleased version's lockfile that is not in canonical form, reading CRLF
  as LF, and accepting a released lockfile in any form the reader accepts (section 4);
- order fingerprints: parse/render/round-trip, malformed and mismatched fingerprints, reorders
  of common type and parameter members caught by `--against`, additions/removals not reported as
  reorders, a v2 lockfile without fingerprints migrating on the next `lock`, and `check_append_only`
  refusing a reordered section;
- order-section completeness: a missing section for an unreleased version, a deleted line, a
  subset, a non-member, an unknown parent, a released legacy section without a marker, a released
  section with a wrong marker, and a direct `diff` accepting a stale order but refusing a
  structurally wrong line;
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

- The type/interface/world inventory for memberless definitions was evaluated and declined for
  0.x/1.0: memberless definitions have no wire identity, and released-directory freezing plus
  generated-output checks guard them.
- Docs-only patch versions were evaluated and declined for 0.x/1.0; see section 2.
- Replacing the lockfile with in-source WIT annotations if component-model issue 695 lands with a
  suitable annotation.

## 16. Declaration-order fingerprints

WIT is positional, but the numbered entries are stored by key, so they cannot show that a
record field or function parameter moved. The order section records that source-level fact
without adding a field to every entry:

```text
# witforge order v1
# order <parent> <fingerprint> = <member> [<member> ...]
```

- `<parent>` is a lockfile parent: a type (`namespace:package/interface/type`), an interface's
  functions (`namespace:package/interface`), or a function's parameters
  (`namespace:package/interface#function`), exactly as in section 3. The interface parent
  fingerprints function declaration order; the function parent fingerprints parameter order.
  The reader parses the parent with the section 3 key grammar and refuses a dotted member key,
  a parameter key, or any other shape that is not one of those three parents.
- Members are the live member names of that parent in declaration order, separated by ASCII
  spaces. Reserved members are absent.
- `<fingerprint>` is exactly sixteen lowercase hexadecimal digits (zero-padded) holding the
  FNV-1a 64 hash over each member name's UTF-8 bytes followed by one NUL byte, in the listed
  order. A leading `+`, uppercase digits, and fifteen or seventeen digits are parse errors. A
  parser MUST verify the fingerprint against the member list.
- The marker must appear before its order lines. A line beginning `# order ` before the marker is
  an ordinary comment. Duplicate markers, duplicate parents, duplicate member names, malformed
  fingerprints, and a fingerprint mismatch are parse errors.
- The section is additive: an old v2 parser ignores it as comments. For an unreleased version the
  section is required and canonical: it carries exactly one line for every parent with live
  members, naming exactly that parent's live members in declaration order. A missing line, a
  line for a parent with no live members, a subset, a non-member, or a wrong order is exit `1`;
  `lock` writes the current section on its next successful reconcile. `check` applies that rule
  in default, `--first`, and `--against` modes, and every generator refuses a lockfile that
  fails it. A released lockfile written before the section existed has no marker and stays
  accepted and unmodified; once a released lockfile has the marker, its section is validated
  exactly like an unreleased one and a mismatch is refused rather than rewritten.
- Direct `diff` endpoints accept a stale order so they can report a reorder, but a line for an
  unknown parent, or one naming a non-member or a subset, is structurally wrong and refused.
  A WIT-root endpoint uses the generator-grade loader and applies the full rule.
- Order discrepancies are collected with entry-level discrepancies and sorted by lockfile key,
  so one report can name every refusal. A non-canonical rendering is reported as canonical-form
  drift before the semantic order check.
- `lock` computes the section from the current live members after every successful reconcile.
  `--first` and `--against` replace the section with the current version's order; they do not
  preserve a draft parent that no longer has live members.
- `--against` compares the order of members common to the current contract and the previous
  lockfile. A reorder of common members fails with exit `1` and writes nothing. Members present
  on only one side are ignored, so adding or removing a member does not report a reorder; a
  reorder that also adds or removes members is still caught for the common members.
- In default mode, `check` and `lock` compare against the current lockfile's section when it is
  present. `check_append_only` (used by the generators between consecutive versions) applies the
  same common-member rule when both sides have the section.
- The order section is not part of wire identity and does not affect generated code or proto
  numbers. `check` includes it in canonical-form checking; an edited section must be regenerated
  by `lock`.

## 17. Reader migration through 0.6.0

The 0.6.0 reader is stricter than the readers through 0.5.0. The following forms used to be
accepted by `WireLock::parse` and are now parse errors (exit `2`, naming the line):

- a kind that does not belong to its key shape, such as an `enum-case` under a `#` function key,
  a `field` under an interface key, or a `method` under a type key;
- a canonical type token outside section 5, including `u8:foo/bar`, a foreign name with too few
  or too many components, a bare `list`/`option`/`result`/`reserved`, and unbalanced, empty, or
  one-slot `list`/`option`/`result` tokens;
- U+00A0, another Unicode space, vertical tab, or form feed used as a field separator;
- an order fingerprint spelled with a leading `+`, uppercase hexadecimal, or fifteen or
  seventeen digits;
- an order parent that is not a type, interface, or function parent in the section 3 key
  grammar;
- a UTF-8 byte-order mark before the header.

Before this tightening, a released lockfile using one of these forms parsed, so freezing it did
not notice the difference. The reader was run over every `wire.lock` in the test fixtures, both
examples, the alint contract fixture, and every release tag; none uses a refused form. A
consumer whose released lockfile does use one must add a new version rather than edit the
released directory: the tightened reader refuses the old root, and a released lockfile is never
rewritten.
