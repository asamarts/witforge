# Structured diagnostics and SARIF

Status: implemented 2026-11-05 (`W8-002`); `WF3003` conversion-impact notes 2026-11-17
(`W11-036`); `WF3004` required-addition blockers 2026-11-18 (`W10-036`). `WF3004` shipped in
0.6.0.

Every command keeps its human output as the default. `--format json` and `--format sarif` replace
the diagnostic stream with a stable machine-readable form. Command-specific reports are
unchanged: `diff --json` remains the diff report, and `diff --format json|sarif` describes its
changes as diagnostics.

## Diagnostic envelope

JSON diagnostics use this versioned envelope:

```json
{
  "version": 1,
  "tool": { "name": "witforge", "version": "0.2.0" },
  "diagnostics": [
    {
      "code": "WF1001",
      "severity": "error",
      "message": "resources and handles are not supported",
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

## SARIF

`--format sarif` emits SARIF 2.1.0. Each diagnostic becomes one result with `ruleId` equal to its
code, `level` equal to its severity, a `message.text`, and, when a path is known, a
`physicalLocation.artifactLocation.uri` (and a `region` when a position is known). The driver
lists the distinct rules used by the run. An empty diagnostic set is a valid SARIF run with an
empty `results` array.

## Stable codes

Codes are never reused for a different meaning.

| Code | Severity | Meaning |
|---|---|---|
| `WF1001` | error | WIT subset violation. The path is the logical contract location. |
| `WF1002` | error | A lockfile belongs to another package. |
| `WF1003` | error | Two names in one scope collide (differ only in ASCII letter case, including against a reserved lockfile member, or map to one generated identifier). |
| `WF1100` | error | Lockfile continuity/drift evaluation failed. |
| `WF1101` | error | A live member has no wire number. |
| `WF1102` | error | A removed member is not reserved in the current lockfile. |
| `WF1103` | error | A removed member was reintroduced under its reserved name/number. |
| `WF1104` | error | A stale draft entry remains in the lockfile. |
| `WF1105` | error | A draft member changed kind or canonical type. |
| `WF1106` | error | The lockfile is not in canonical `lock` form. |
| `WF1107` | note | The pinned dependency graph cannot be compared because the baseline is not a version-directory lockfile. |
| `WF1108` | error | A draft removal can be restored; `lock --against` would un-reserve the member. |
| `WF1201` | note | `lock` added a member. |
| `WF1202` | note | `lock` reserved a removed member. |
| `WF1203` | note | `lock` dropped a stale draft. |
| `WF1204` | note | `lock` updated a draft's kind or type. |
| `WF1205` | note | `lock` restored a draft removal. |
| `WF2001` | error | A generated file is missing, changed, extra, unowned, or otherwise refuses `--check`. |
| `WF3001` | error | `diff` found a wire-, source-, or generated-API-breaking change. |
| `WF3002` | note | `diff` found a compatible change. |
| `WF3003` | note | `diff` found a record whose generated new-to-old conversion is guaranteed to fail. The path is the missing member. |
| `WF3004` | note | `diff` found a non-defaulted required record field addition that makes conversion generation refuse. The path is the added field. |
| `WF9000` | error | A usage, I/O, parse, released-directory, or internal command error without a more specific code. |

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
failure, and `2` usage, I/O, parse, or internal failure.

`diff --format json|sarif` emits each diff change as a diagnostic (`WF3001` for breaking,
`WF3002` for compatible), each guaranteed conversion-impact note as `WF3003`, and each
required-addition generation blocker as `WF3004`; it suppresses the human diff report.
`diff --json` remains the full
diff report described in [`diff.md`](diff.md); when both `--json` and a non-human `--format`
are given, the report wins and the diagnostic envelope is suppressed.
