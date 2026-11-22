# Structured diagnostics and SARIF

Status: implemented 2026-11-05 (`W8-002`); `WF3003` conversion-impact notes 2026-11-17
(`W11-036`); `WF3004` required-addition blockers 2026-11-18 (`W10-036`); per-rule subset codes,
cross-command parity, usage-error reporting, and internal-only `WF9000` 2026-11-21 (`W10-079`).

Every command keeps its human output as the default. `--format json` and `--format sarif` replace
the diagnostic stream with a stable machine-readable form. Command-specific reports are
unchanged: `diff --json` remains the diff report, and `diff --format json|sarif` describes its
changes as diagnostics.

## Diagnostic envelope

JSON diagnostics use this versioned envelope:

```json
{
  "version": 1,
  "tool": { "name": "witforge", "version": "0.6.0" },
  "diagnostics": [
    {
      "code": "WF1015",
      "severity": "error",
      "message": "resources and handles are not supported; contract operations are pure functions over values",
      "path": "example:plugin/types/blob"
    }
  ]
}
```

Fields:

| Field | Meaning |
|---|---|
| `code` | Stable diagnostic code from the catalogue below. |
| `severity` | `error`, `warning`, or `note`. |
| `message` | Human-readable explanation. |
| `path` | Contract, source, member, generated-file, or lockfile path when the source identifies one. |
| `line`, `column`, `end_line`, `end_column` | One-based source range when a source position is available. |

JSON output is printed to stdout. Human diagnostics continue to go to stderr and successful
human command output remains unchanged.

A contract violation emits the same diagnostic code, path, and message whichever command reports it
(`check`, `lock`, `diff`, `rust`, `proto`, `schema`, or `glue`). Multiple violations remain
separate diagnostics in the `diagnostics` / SARIF `results` array and are never joined into one
message.

## SARIF

`--format sarif` emits SARIF 2.1.0. Each diagnostic becomes one result with `ruleId` equal to its
code, `level` equal to its severity, a `message.text`, and, when a path is known, a
`physicalLocation.artifactLocation.uri` (and a `region` when a position is known). The driver
lists the distinct rules used by the run. An empty diagnostic set is a valid SARIF run with an
empty `results` array.

## Stable codes

Codes are never reused for a different meaning. Every code below is registered in
`witforge::diagnostic::CATALOG` and verified by automated parity tests against this table.

In `W10-079`, subset violations previously reported under the single umbrella code `WF1001` were
split into one code per subset rule (`WF1010`–`WF1030`). The **Prior** column records `WF1001`
beside each split code for consumers updating JSON or SARIF filters.

| Code | Severity | Prior | Meaning |
|---|---|---|---|
| `WF1001` | error | — | Unrecognised WIT construct outside the supported subset (legacy umbrella code; specific subset rules now emit `WF1010`–`WF1030`). |
| `WF1002` | error | — | A lockfile belongs to another package. The path names the lockfile holding the foreign key. |
| `WF1003` | error | — | Two names in one scope collide (differ only in ASCII letter case, including against a reserved lockfile member, or map to one generated identifier, module, filename, accessor, or reserved target name). |
| `WF1010` | error | `WF1001` | Root contract package version is missing or not `<major>.<minor>.0`. |
| `WF1011` | error | `WF1001` | Nested `package` block inside a version directory; dependencies must live under `deps/`. |
| `WF1012` | error | `WF1001` | Dependency package under `deps/` has a missing or unsupported version (must be `<major>.<minor>.0`). |
| `WF1013` | error | `WF1001` | Duplicate dependency package identity or dependency sharing the root package's `namespace:name`. |
| `WF1014` | error | `WF1001` | Unsupported WIT annotation (such as `@external-id`; only `@since`, `@unstable`, and `@deprecated` are accepted). |
| `WF1015` | error | `WF1001` | Resources and handles (`resource`, `borrow<T>`, `own<T>`) are not supported. |
| `WF1016` | error | `WF1001` | `future` and `stream` types are not supported. |
| `WF1017` | error | `WF1001` | `char` primitive is not supported; use `string`. |
| `WF1018` | error | `WF1001` | `error-context` is not supported. |
| `WF1019` | error | `WF1001` | Tuples are not supported; use a named record. |
| `WF1020` | error | `WF1001` | `map` is not supported; use `list<record>`. |
| `WF1021` | error | `WF1001` | Fixed-length lists are not supported; use `list`. |
| `WF1022` | error | `WF1001` | Empty records are not supported. |
| `WF1023` | error | `WF1001` | `async` functions are not supported. |
| `WF1024` | error | `WF1001` | `get` and `set` accessors are not supported. |
| `WF1025` | error | `WF1001` | Inline interfaces in a world are not supported. |
| `WF1026` | error | `WF1001` | Named (renamed) world interface imports or exports are not supported. |
| `WF1027` | error | `WF1001` | Functions defined directly in a world are not supported. |
| `WF1028` | error | `WF1001` | Types defined directly in a world are not supported. |
| `WF1029` | error | `WF1001` | World imports or exports an interface with no package or from outside `deps/`. |
| `WF1030` | error | `WF1001` | Type nesting, alias chain, canonical type length, synthesized wrapper name length, or canonical type cycle limit exceeded. |
| `WF1031` | error | — | Version directory name (`v<major>.<minor>`) disagrees with its declared package version (`<major>.<minor>.0`). |
| `WF1032` | error | — | Versions in a WIT root or `diff` endpoints declare different package identities (`namespace:name`). |
| `WF1033` | error | — | Pinned dependency graphs differ across versions, `diff` endpoints, or `--against` baseline. |
| `WF1100` | error | — | Lockfile continuity, drift evaluation, order-section, or baseline-policy violation. |
| `WF1101` | error | — | A live member has no wire number. |
| `WF1102` | error | — | A removed member is not reserved in the current lockfile. |
| `WF1103` | error | — | A removed member was reintroduced under its reserved name/number. |
| `WF1104` | error | — | A stale draft entry remains in the lockfile. |
| `WF1105` | error | — | A draft member changed kind or canonical type. |
| `WF1106` | error | — | The lockfile is not in canonical `lock` form. |
| `WF1107` | note | — | The pinned dependency graph cannot be compared because the baseline is not a version-directory lockfile. |
| `WF1108` | error | — | A draft removal can be restored; `lock --against` would un-reserve the member. |
| `WF1201` | note | — | `lock` added a member. |
| `WF1202` | note | — | `lock` reserved a removed member. |
| `WF1203` | note | — | `lock` dropped a stale draft. |
| `WF1204` | note | — | `lock` updated a draft's kind or type. |
| `WF1205` | note | — | `lock` restored a draft removal. |
| `WF2001` | error | — | A generated file is missing, changed, extra, unowned, or otherwise refuses `--check`. |
| `WF2002` | error | — | Version-to-version Rust conversion generation refused an incompatible evolution between adjacent versions. |
| `WF3001` | error | — | `diff` found a wire-, source-, or generated-API-breaking change. |
| `WF3002` | note | — | `diff` found a compatible change. |
| `WF3003` | note | — | `diff` found a record whose generated new-to-old conversion is guaranteed to fail. The path is the missing member. |
| `WF3004` | note | — | `diff` found a non-defaulted required record field addition or retyped member that makes conversion generation refuse. The path is the affected member. |
| `WF9000` | error | — | Internal compiler error (unexpected invariant failure only; never emitted for usage, I/O, parse, or guard errors). |
| `WF9001` | error | — | Command-line usage or argument error (invalid flags, missing required options, WIT root passed to `check`/`lock`, reversed `diff` endpoints, invalid module root). |
| `WF9002` | error | — | Filesystem or I/O error (missing file, directory, or lockfile; permission denied; unreadable path; no version directories under root; write failure). |
| `WF9003` | error | — | Parse or format error (invalid WIT source or resolution error, malformed `wire.lock`, malformed `released` manifest, or invalid `witforge.toml` / BOM). |
| `WF9004` | error | — | Safety guard or freeze-policy refusal (locking or writing into a released directory, symlinked version directory or symlink in a released directory, overlapping/unowned/foreign/symlink output path, non-canonical or duplicate `v...` entry, or unexpanded shell path in `witforge.toml`). |

## Source positions

WIT subset diagnostics carry the logical contract path, for example
`example:plugin/types/finding.span`. The owned model deliberately keeps no `wit-parser` spans
(see [`model.md`](model.md) section 5), so those diagnostics do not currently have physical
line/column positions. Generated-output diagnostics carry the filesystem path, and lockfile
diagnostics carry a lockfile path or member key. The envelope has optional position fields so a
source-aware diagnostic can add them without changing its code. The SARIF region is emitted only
when a position is present.

## Scope and exit codes

`check`, `lock`, `diff`, `rust`, `proto`, `schema`, and `glue` accept `--format`. The default is
`human`, so existing human output is preserved byte-for-byte for the existing messages. Exit
codes do not depend on the format: `0` success, `1` contract/lockfile/output or breaking-change
failure, and `2` usage, I/O, parse, guard, or internal failure.

When argument parsing itself fails with `--format json` or `--format sarif` present in the raw
arguments, witforge emits a JSON or SARIF report containing a `WF9001` error on stdout and still
exits with `2`. `--help` and `--version` continue to print text to stdout and exit `0`.

`diff --format json|sarif` emits each diff change as a diagnostic (`WF3001` for breaking,
`WF3002` for compatible), each guaranteed conversion-impact note as `WF3003`, and each
required-addition or retype generation blocker as `WF3004`; it suppresses the human diff report.
`diff --json` remains the full diff report described in [`diff.md`](diff.md); when both `--json`
and a non-human `--format` are given, `--json` wins and the diagnostic envelope is suppressed
(including on command errors, which are written to stderr just as `diff --json` does).
