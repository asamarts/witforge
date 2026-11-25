---
layout: default
title: Diagnostic codes
---

# Diagnostic codes

Every `witforge` command emits structured diagnostics with `--format json` or `--format sarif`.
SARIF rules link directly to the per-code anchors on this page, and `witforge explain <CODE>`
prints any code's catalogue entry offline from the CLI.

Positions use **1-based lines and 1-based UTF-16 code-unit columns** (the SARIF 2.1.0 default).
Physical paths in JSON `file` fields and SARIF `artifactLocation.uri` are repository-relative
with forward slashes when inside the working directory; SARIF uses an absolute `file:///` URI for
files outside the working directory. Logical WIT paths (such as `example:pkg/types/record.field`)
appear in `logical` (JSON) and `logicalLocations` (SARIF).

## Subset and contract codes (`WF1001`–`WF1033`)

### WF1001

**Severity:** `error`

Unrecognised WIT construct outside the supported subset. Legacy umbrella code; individual subset
rules now emit `WF1010`–`WF1030`.

### WF1002

**Severity:** `error`

A lockfile belongs to another package (`namespace:name`). Check the `wire.lock` or `--against`
baseline path named by the diagnostic.

### WF1003

**Severity:** `error`

Two names in one scope collide: either they differ only in ASCII letter case (including against a
reserved lockfile member), or they map to the same generated Rust, protobuf, `prost`, or glue
identifier, filename, accessor, or reserved name. Rename one of the colliding items.

### WF1010

**Severity:** `error` · **Prior:** `WF1001`

Root contract package version is missing or not `<major>.<minor>.0`. Declare
`package ns:name@<major>.<minor>.0;` with patch `0` and no pre-release or build metadata.

### WF1011

**Severity:** `error` · **Prior:** `WF1001`

A nested `package` block was found inside a version directory. Place dependency packages under
the standard `deps/` directory instead.

### WF1012

**Severity:** `error` · **Prior:** `WF1001`

A dependency package under `deps/` is missing a version or uses a version other than
`<major>.<minor>.0`.

### WF1013

**Severity:** `error` · **Prior:** `WF1001`

Duplicate dependency package identity under `deps/`, or a dependency declaring the same
`namespace:name` as the root contract package. Pin one version per dependency package.

### WF1014

**Severity:** `error` · **Prior:** `WF1001`

Unsupported or invalid WIT stability annotation. Only `@since`, `@unstable`, and `@deprecated` are
accepted, and `@deprecated` must accompany `@since` or `@unstable`. Annotation versions must use
`<major>.<minor>.0`, cannot be later than the containing package version, cannot have `@deprecated`
predate `@since`, and must keep `@since` aligned across multi-version contracts.

### WF1015

**Severity:** `error` · **Prior:** `WF1001`

Resources and handles (`resource`, `borrow<T>`, `own<T>`) are not supported. Model state as pure
values passed to freestanding functions.

### WF1016

**Severity:** `error` · **Prior:** `WF1001`

`future` and `stream` types are not supported; witforge contracts are synchronous.

### WF1017

**Severity:** `error` · **Prior:** `WF1001`

`char` is not supported; use `string`.

### WF1018

**Severity:** `error` · **Prior:** `WF1001`

`error-context` is not supported; model errors as explicit records, enums, or variants in
`result<T, E>`.

### WF1019

**Severity:** `error` · **Prior:** `WF1001`

Tuples are not supported; define a named `record` so every field has a stable wire number.

### WF1020

**Severity:** `error` · **Prior:** `WF1001`

`map` is not supported yet; use `list<record>` with explicit key and value fields.

### WF1021

**Severity:** `error` · **Prior:** `WF1001`

Fixed-length lists are not supported; use `list<T>`.

### WF1022

**Severity:** `error` · **Prior:** `WF1001`

Empty records are not supported; add a field or use a payload-less variant case.

### WF1023

**Severity:** `error` · **Prior:** `WF1001`

`async` functions are not supported; witforge contracts are synchronous.

### WF1024

**Severity:** `error` · **Prior:** `WF1001`

`get` and `set` accessors are not supported; use ordinary functions.

### WF1025

**Severity:** `error` · **Prior:** `WF1001`

Inline interfaces in a `world` are not supported; declare a named `interface` and import or export
it from the world.

### WF1026

**Severity:** `error` · **Prior:** `WF1001`

Named (renamed) interface imports or exports in a `world` are not supported; import or export the
interface directly without a local alias.

### WF1027

**Severity:** `error` · **Prior:** `WF1001`

Functions defined directly in a `world` are not supported; place functions inside a named
interface and export that interface from the world.

### WF1028

**Severity:** `error` · **Prior:** `WF1001`

Types defined directly in a `world` are not supported; define types inside a named interface.

### WF1029

**Severity:** `error` · **Prior:** `WF1001`

A `world` imports or exports an interface with no package or from a package outside `deps/`.

### WF1030

**Severity:** `error` · **Prior:** `WF1001`

Type nesting depth, alias chain depth, canonical type byte length, synthesized protobuf wrapper
name length, or canonical type cycle limit was exceeded.

### WF1031

**Severity:** `error`

Version directory name (`v<major>.<minor>`) disagrees with its declared WIT package version
(`<major>.<minor>.0`).

### WF1032

**Severity:** `error`

Versions in a WIT root or the two endpoints of `diff` declare different package identities
(`namespace:name`).

### WF1033

**Severity:** `error`

Pinned dependency graphs differ across versions under a WIT root, across `diff` endpoints, or
against a version-directory `--against` baseline.

## Lockfile codes (`WF1100`–`WF1205`)

### WF1100

**Severity:** `error`

Lockfile continuity, drift evaluation, order-section, or baseline-policy violation. When a live
member disagrees with its locked entry, the related location points at the conflicting line in
`wire.lock`.

### WF1101

**Severity:** `error`

A live contract member has no wire number in `wire.lock`. Run the suggested `witforge lock`
command to assign one.

### WF1102

**Severity:** `error`

A removed contract member is still unreserved in `wire.lock`. Run `witforge lock` to mark it
`reserved`.

### WF1103

**Severity:** `error`

A removed member was reintroduced under its reserved name and wire number. Give the member a new
name (or use `--against` with the released baseline if the removal was only a draft).

### WF1104

**Severity:** `error`

A stale draft entry remains in `wire.lock`. Run `witforge lock` to drop it.

### WF1105

**Severity:** `error`

A draft member changed kind or canonical type since `wire.lock` was written. Run `witforge lock`
to update the draft entry.

### WF1106

**Severity:** `error`

`wire.lock` is not in the canonical formatting produced by `witforge lock`. Run `witforge lock` to
rewrite it canonically.

### WF1107

**Severity:** `note`

The pinned dependency graph could not be compared because `--against` does not point to a
version-directory `wire.lock`.

### WF1108

**Severity:** `error`

A draft removal can be restored; running `witforge lock --against` will un-reserve the member.

### WF1109

**Severity:** `error`

A released version directory listed in `wit/released` changed after release, had an entry removed
from `wit/released`, contains a symlink or untracked file, or lacks a git-tracked `wire.lock` under
its exact spelling. Restore the released directory and manifest rather than editing a released
version in place.

### WF1201

**Severity:** `note`

`witforge lock` assigned a wire number to a newly added member.

### WF1202

**Severity:** `note`

`witforge lock` marked a removed member as `reserved`.

### WF1203

**Severity:** `note`

`witforge lock` dropped a stale draft entry.

### WF1204

**Severity:** `note`

`witforge lock` updated a draft member's kind or canonical type.

### WF1205

**Severity:** `note`

`witforge lock` restored a draft removal.

## Generation codes (`WF2001`–`WF2002`)

### WF2001

**Severity:** `error`

Generated output is missing, changed, extra, or otherwise out of sync with `--check`. Re-run the
suggested generator command.

### WF2002

**Severity:** `error`

Version-to-version Rust conversion generation (`convert.rs`) refused an incompatible evolution
between adjacent versions (such as adding a non-defaulted record field or retyping a member).

## Diff codes (`WF3001`–`WF3004`)

### WF3001

**Severity:** `error` (breaking a gated view under `--check[=<views>]`) or `warning` (un-gated)

`witforge diff` found a wire-, source-, generated-API-, or decode-breaking change. Without `--check`,
`diff` exits `0` and emits `WF3001` at level `warning`; with `--check` or `--check=<views>`,
`diff` exits `1` and emits `WF3001` at level `error` for changes breaking a gated view (and
`warning` for breaking changes outside the gated views).

### WF3002

**Severity:** `note`

`witforge diff` found a compatible change.

### WF3003

**Severity:** `note`

`witforge diff` found a record whose generated new-to-old conversion is guaranteed to fail because
the older version requires a member removed in the newer version.

### WF3004

**Severity:** `note`

`witforge diff` found a non-defaulted record field addition, retyped member, or cross-major
conversion omission that will cause `convert.rs` generation to refuse or omit that item.

## Operational and internal codes (`WF9000`–`WF9004`)

### WF9000

**Severity:** `error`

Internal compiler error (unexpected invariant failure only; never emitted for usage, I/O, parse,
or safety-guard errors).

### WF9001

**Severity:** `error`

Command-line usage or argument error (invalid flags, missing required arguments, reversed `diff`
endpoints, invalid module root). Exits `2`.

### WF9002

**Severity:** `error`

Filesystem or I/O error (missing file, directory, or lockfile; permission denied; unreadable path;
write failure). Exits `2`.

### WF9003

**Severity:** `error`

Parse or format error (invalid WIT syntax or name resolution error, malformed `wire.lock`,
malformed `released` manifest, or invalid `witforge.toml` / BOM). Exits `2`.

### WF9004

**Severity:** `error`

Safety guard or freeze-policy refusal (locking or writing into a released directory, symlinked
version directory or symlink inside a released directory, overlapping or unowned output path,
non-canonical `v...` directory name, or unexpanded shell path in `witforge.toml`). Exits `2`.
