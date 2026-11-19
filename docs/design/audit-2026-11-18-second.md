# Second repository audit: 2026-11-18

Status: audit recorded 2026-11-18 on `main` at `6565490`, 51 commits after tag `v0.5.0`. It
follows [the first audit of the same day](audit-2026-11-18.md), whose remediation tasks
`W10-026` to `W10-037` were already closed. Every finding below maps to a task, and the plan,
not this record, says which of them are done: `aplan show <id>` for one task, `aplan ready` for
what can start. This is an internal design note;
`docs/_config.yml` excludes the `design/` tree from the public site.

## Summary

The first audit checked that the repository does what its tests and documents say. This one
tested the two properties the tool exists to provide, using contracts the suite does not
contain. Neither holds for ordinary input:

1. **Wire identity can change while every gate passes.** A released type can change kind, a
   byte list can change encoding, two versions of a dependency are treated as one, and `diff`
   reports "no contract changes" for a renumbered interface (findings A1 to A6).
2. **Generated code can fail to compile while every command exits 0.** A one-letter or
   upper-case word in a WIT name, a structural alias in a dependency, or a field named `extern`
   is enough (findings B1 to B9).

The suite did not see either because its golden files are written by the emitter under test and
its compile tests use lower-case, multi-letter names in single-file contracts (finding D5). The
only consumer, alint's proof crate, has one version in one file with no dependencies, enums, or
aliases, so it exercises none of these paths (finding D6).

The tool itself also loses work: `lock` piped into `head` writes no lockfile, two generators in
one output directory delete each other's files, and a multi-file contract generates in directory
enumeration order (findings C1 to C4). The published Action ignores its `diff` inputs, and the
next release cannot pass its own version check (findings E1 and E2).

The tables below hold 70 findings: 3 critical, 22 high, 34 medium, 10 low, and one row of ideas
that are not defects. Many rows group several related defects. They are recorded as 61 tasks in
the `M-p10` cut (`W10-038` to `W10-098`) and 2 in `M-p11`. One of them, a 0.5.1 patch release
(`W10-044`), was dropped the same day by the owner's decision: everything ships in 0.6.0.

## Scope and method

The audit covered the library and CLI source, the three normative specifications and the other
design documents, the public documentation, the examples, the Action, the CI and release
workflows, the fuzz workspace, and the plan. Five independent reviews (core model and lockfile,
emitters, CLI and diagnostics, release and CI, documentation) produced candidate findings; each
was then re-run or re-read, and the table says which.

Verification labels:

- **re-run**: reproduced by the auditor with the built `witforge` binary, and for compile
  failures in a scratch crate against prost 0.14 next to a control that compiles.
- **reviewer-run**: reproduced by a reviewing agent with a recorded command; not repeated.
- **read**: established by reading code, workflows, or documents; nothing was executed.

No product code was changed. The audit added this document, the tasks, one dependency edge
(`W10-002` now waits for `W10-069` and `W10-098`), and pointers in `ROADMAP.md` and the first
audit record.

## Gate matrix

All gates that were run pass, which is the point of finding D5.

| Gate | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace --locked` (with `TMPDIR` under `~/.tmp`) | pass, 230 tests |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | pass |
| `ci/check-generated.sh` | pass |
| `ci/check-release-freeze.sh crates/witforge/tests/fixtures/versions/wit` | pass |
| aplan `check`, `verify`, `export-agents-md --check`, `check-docs` | pass |

Not re-run in this audit: the MSRV suite, `ci/check-security.sh`, `cargo semver-checks`, the
fuzz build, actionlint, shellcheck, `node --test action/test.js`, and coverage.

## Findings and task map

Severity: **critical** is a silent wire change in a workflow where every documented gate passes;
**high** is silent wrong output, data loss, a build failure in the consumer, or a blocked
release; **medium** is a wrong or missing diagnostic, a specification mismatch, or a hardening
gap; **low** is polish.

### A. Wire identity, lockfile, and diff

| # | Severity | Verified | Finding | Task |
|---|---|---|---|---|
| A1 | critical | re-run | A released type changes kind (enum to record, and the eleven other pairs) with new member names; `lock --against`, `check --against`, `proto`, `glue`, and `schema` pass, `diff` says `wire:compatible`, and the referring field changes from an enum to a message. Only `rust` refuses. | `W10-045` |
| A2 | critical | re-run | `list<byte>` with `type byte = u8` is `repeated uint32`; `list<u8>` is `bytes`; both lock as `list<u8>`, so respelling changes the encoding under an unchanged lockfile. | `W10-046` |
| A3 | critical | re-run | Two versions of one dependency package are one package to the lockfile and emitters; `proto` exits 0 and types the 1.0.0 field with the 0.1.0 message. | `W10-047` |
| A4 | high | re-run | `diff` does not compare method ids (a renumbered interface is "no contract changes … COMPATIBLE" while `proto` exits 1) or reserved-number reuse (reviewer-run); `lock --first` is accepted in a minor that has a predecessor. | `W10-048` |
| A5 | high | re-run | The order section is not validated: with its lines deleted and released fields and parameters reversed, the default `check`, `rust`, and `proto` exit 0 and the generated signature is swapped. `check --first` skips the comparison. | `W10-049` |
| A6 | high | re-run | Wrapper message names are not injective and are reused by name: two different nested list types share `WitforgeListOfListOfAbCd`. | `W10-050` |
| A7 | medium | re-run; removed-interface case reviewer-run | Reserved entries are missing from proto output after a kind change, and for the methods of a removed interface. | `W10-045` |
| A8 | medium | re-run | Canonical types record a dependency alias by name, against `lockfile.md` section 5. A retargeted alias passes `check --against`; the generators catch it through the pinned-dependency-graph rule. | `W10-051` |
| A9 | medium | re-run | `check` reports the first released discrepancy only; the specification says every one. | `W10-052` |
| A10 | medium | re-run for kind and key shape; rest reviewer-run | The lockfile reader accepts entries whose kind does not fit the key, malformed type tokens, U+00A0 separators, and loose order parents. | `W10-053` |
| A11 | medium | re-run | A removal made in an unreleased minor cannot be undone, although the member is live in the released baseline. | `W10-054` |
| A12 | medium | re-run | A dependency version violation exits 2 without a package name, file, or code, even for a dependency nothing uses. | `W10-055` |
| A13 | low | re-run for the named import; rest reviewer-run | `import foo: types;` is reported as an inline interface; `@external-id` is dropped; a world's implicit import is not mentioned; case-variant reintroduction passes `check` and fails in `proto`. | `W10-055` |
| A14 | low | re-run | `diff` does not read `wit/released`: a symlinked `wire.lock` that `check` refuses is accepted. | `W10-056` |
| A15 | medium | re-run | `diff` exits 2 ("type is not in the model") for a field typed by a dependency alias of a named dependency type, even comparing a version with itself. Aliases of primitives and of lists work. | `W10-056` |
| A16 | high | re-run | Replacing an alias by its target passes `check --against` and `diff --check`, and `rust` refuses to generate. | `W10-057` |
| A17 | medium | re-run at small sizes; large sizes reviewer-run | An alias chain 8,000 deep overflows the stack; 16 lines of WIT produce 12.9 MB of glue, growing about 4.7 times per two extra lines. | `W10-058` |
| A18 | low | read | Specification contradictions: number reuse (`overview.md` 5.2 against `lockfile.md` 6), a wrong cross-reference, and `diff.md` treating a kind change as wire-compatible. | `W10-043`, `W10-045` |

### B. Generated output

Every row is an exit 0 from `witforge` unless it says otherwise.

| # | Severity | Verified | Finding | Task |
|---|---|---|---|---|
| B1 | high | re-run | A one-letter word in a type, case, or function name (`x-kind`, `get-x-y`) makes glue name identifiers prost does not generate (E0599, E0425, E0422). | `W10-059` |
| B2 | high | re-run | A named `list` or `option` type that nothing refers to gets glue but no proto wrapper (E0425). | `W10-060` |
| B3 | high | re-run | A dependency's nested list alias: the wrapper is defined in the root's proto package and looked up in the dependency's (E0425, E0422). | `W10-061` |
| B4 | high | re-run | A dependency's `list<u8>` alias used as a list element or option payload produces glue that does not parse or type-check. | `W10-061` |
| B5 | medium | re-run for the emitted text; protoc failure reviewer-run | Cross-package proto references have no leading dot, so a root named like a dependency's namespace emits an unresolvable reference. | `W10-062` |
| B6 | medium | re-run | A member named `extern` is `r#extern` in glue and `extern_` in prost (E0560). | `W10-063` |
| B7 | medium | re-run | `--plain-root` and `--proto-root` accept `super::…` and `self::…`, which cannot be right at every module depth. | `W10-064` |
| B8 | medium | reviewer-run | A foreign named option in a list or oneof position accepts an unset wrapper as `None`; other paths return `InvalidState`. | `W10-065` |
| B9 | medium | re-run for fields and cases; rest reviewer-run | Upper-case WIT words (`foo-BAR`, `HTTP`) mismatch prost (E0560, E0599) and give non-snake-case plain Rust; `interface API` is refused by `rust` only. | `W10-059`, `W10-067` |
| B10 | medium | re-run for the three named cases; rest reviewer-run | Glue exits 0 for a dependency `proto` refuses; `mode` next to `set-mode` collides with prost's accessor (E0592); a package named `service` passes `check` and fails `proto`; unused proto imports; keyword package segments. | `W10-062`, `W10-063`, `W10-065`, `W10-067` |
| B11 | medium | re-run; carriage return and missing comments reviewer-run | WIT comments are copied verbatim: an indented example becomes a failing doctest, `list<string>` fails `rustdoc -D warnings`. | `W10-066` |
| B12 | medium | re-run | An unused unsupported dependency type passes `check`; `rust`, `proto`, and `schema` then exit 2 with an internal error. | `W10-067` |
| B13 | high | re-run | JSON Schema cannot express the payload-less side of `result<_, E>`, `result<T>`, or `result`; the specification asks for exactly this shape. | `W10-070` |
| B14 | medium | reviewer-run | `--serde`: `Some(None)` and NaN do not round-trip; the JSON names are unspecified and differ from the JSON Schema emitter's. | `W10-071` |
| B15 | medium | re-run for rustfmt and `missing_docs`; rest reviewer-run | `convert.rs` breaks when the consumer adds a `From` impl for `ConversionError`; generated Rust fails `cargo fmt --check` and `missing_docs`; new-to-old conversion drops fields silently. | `W10-072`, `W10-074` |
| B16 | low | re-run | A type-only interface gets an empty trait and an empty method enum; unit functions are `-> ()`. | `W10-074` |

### C. Tool behaviour

| # | Severity | Verified | Finding | Task |
|---|---|---|---|---|
| C1 | high | re-run on one file system | Output of a multi-file version directory follows directory enumeration order; renaming files changes the bytes. A difference between machines is inferred, not demonstrated. | `W10-077` |
| C2 | high | re-run | A closed stdout panics with exit 101; `lock` prints before it writes, so `lock … \| head -1` left no `wire.lock` in 30 of 30 runs with 400 members (0 of 30 with 6). | `W10-078` |
| C3 | high | re-run | The guard against writing into the WIT root is bypassed by `missing/../symlink`. | `W10-075` |
| C4 | high | re-run | Rust, proto, and glue share one ownership header, so generators or roots that share an output directory delete each other's files; two documents say the headers differ. | `W10-073` |
| C5 | medium | re-run; cross-command cases reviewer-run | In JSON and SARIF, a generator reports contract violations as one `WF9000` internal error without a path; usage errors leave stdout empty; all subset rules share `WF1001`. | `W10-079` |
| C6 | medium | re-run for the location; rest read | SARIF puts a logical name in `artifactLocation.uri` and never emits a region; levels and exit status disagree for `diff`. | `W10-080` |
| C7 | medium | re-run | WIT syntax and resolution errors name the directory, not the file or line. | `W10-080` |
| C8 | low | re-run for the swap; rest reviewer-run | A reversed `diff` prints the two versions swapped; remedies name a bare `witforge lock`; `check` on a root gives no hint. | `W10-081` |
| C9 | medium | re-run; overflow and BOM reviewer-run | Directories named `v0.2.0`, `V0.2`, or `v0.2-rc1` are ignored silently; `~` in a configuration path creates a directory named `~`. | `W10-082` |
| C10 | low | re-run for mode and rewrite; rest reviewer-run | Regeneration resets a file's mode and rewrites identical files; an unreadable unrelated file aborts the run. | `W10-076` |
| C11 | low | re-run | `--check` says only "changed generated file". | `W10-076` |
| C12 | medium | read | The release-freeze check exists only as a script in this repository; there is no whole-root `check`, `generate`, `new-version`, configuration discovery, quiet mode, or build identity in `--version`. | `W10-083`, `W10-084`, `W10-085` |

### D. Design, public API, and tests

| # | Severity | Verified | Finding | Task |
|---|---|---|---|---|
| D1 | high | re-run | With two majors in one root, `rust` refuses the breaking changes a major exists for; the working arrangement (separate roots) is undocumented; a version cannot be retired. | `W10-093` |
| D2 | medium | re-run | `@deprecated`, `@since`, and `@unstable` reach no output, and the model has no deprecated state. | `W10-094` |
| D3 | high | read | No model type is `#[non_exhaustive]`, so every planned model addition is a major library release after 1.0; the library returns strings where the CLI needs structure; `check` and `lock` exist only in the binary. | `W10-095`, `W10-096` |
| D4 | high | read | `diff` calls an added required field or enum case wire-compatible, and the generated glue rejects the messages (`MissingField`, `UnknownEnumValue`). | `W10-097` |
| D5 | high | read | The suite cannot see findings B1 to B12: golden files are self-referential, compile tests use few shapes and names, example output is not compiled, and no test states "check passes, so generation succeeds and compiles". | `W10-068`, `W10-089` |
| D6 | high | read | The 1.0 gate rests on adoption evidence from a consumer that exercises a narrow slice and no evolution. | `W10-098` |
| D7 | idea | — | Trait adapters between versions; proto file options for other languages. The cross-language profile is already `W11-034`. | `W11-038`, `W11-039` |

### E. Release, CI, and the Action

| # | Severity | Verified | Finding | Task |
|---|---|---|---|---|
| E1 | high | read, against GitHub's reference and `@actions/core` | The Action reads `INPUT_DIFF_OLD`; the runner sets `INPUT_DIFF-OLD`. Every hyphenated input is empty, so the documented `diff` gate never runs. | `W10-039` |
| E2 | high | read; simulated by a reviewer | The version-reference check added after 0.5.0 requires the CI Action pin to equal the workspace version, which cannot be true before the release exists, and the release requires that CI to pass. | `W10-040` |
| E3 | high | re-run | The roadmap lists five changes under "0.5.0 (released)" that are not in the tag; the changelog omits three user-visible changes; `main` reports `0.5.0`. | `W10-043`, `W10-069` |
| E4 | medium | re-run | The semver baseline and the buf comparison stop at 0.4.1; four later releases are unprotected. | `W10-041` |
| E5 | medium | read | An Action pinned by SHA or branch installs the latest binary; it has no `against`, freeze, or format input and no `linux/arm64`. | `W10-086` |
| E6 | medium | read | crates.io is published before platform builds; the GitHub release is public before its assets exist; `--clobber` can replace an asset; there is no provenance. | `W10-087` |
| E7 | medium | read | Pending runs on `main` can be cancelled; nothing runs on a schedule; the lint toolchain floats; scripts default to `/tmp`; benchmark ceilings are 56 to 5,000 times the measured values. | `W10-088` |
| E8 | medium | re-run for the corpus | The WIT fuzz target has no tracked corpus or dictionary; local corpora hold inputs of at most 4 bytes, so the emitters were not reached; results are discarded. | `W10-089` |
| E9 | low | reviewer-read | Archives lack licence files; crate metadata is incomplete; `SECURITY.md` has no threat model. | `W10-090` |

### F. Documentation

| # | Severity | Verified | Finding | Task |
|---|---|---|---|---|
| F1 | high | re-run | The quickstart's first example declares an interface and a world with one name and fails at the first command. | `W10-042` |
| F2 | medium | read | The evolution sequences omit creating the directory, copying the WIT, and changing the package version. | `W10-042` |
| F3 | medium | read | `AGENTS.md` describes the 0.1 line and gives a test command that writes to `/tmp`. | `W10-043` |
| F4 | medium | read | Stale or contradictory statements across the specifications and guides; the task lists each one. | `W10-043` |
| F5 | medium | re-run for the `diff` exit status | The documented Action snippet fails on `examples/evolved` once the inputs work; four inputs are documented only in a page excluded from the site. | `W10-086` |
| F6 | medium | read | No guide for wiring generated code into a crate, installing a binary, migrating between releases, or contributing. | `W10-091`, `W10-092` |
| F7 | low | read | Design documents describe `main` while the README installs 0.5.0; the first audit record says no remediation remains. | `W10-043` |
| F8 | low | re-run | 29 documentation links end in `.html` and are 404 when read on GitHub. | `W10-092` |

## Confirmed probes

The task bodies carry the full reproductions. The shortest forms of the most serious ones:

### Kind change of a released type (A1)

```text
v0.1 (released)   enum color { red, green }     record r { c: color }
v0.2              record color { hue: u32 }     record r { c: color }

lock --against, check --against, proto, glue, schema    exit 0
diff                                                     kind-changed … wire:compatible
v0.proto                                                 r.c = 1 is now a message
```

### Byte-list spelling (A2)

```text
type byte = u8;  record r { a: list<byte>, b: list<u8> }
wire.lock   r.a = 1 field list<u8>     r.b = 2 field list<u8>
v0.proto    repeated uint32 a = 1;     bytes b = 2;
```

### Renumbered methods (A4)

```text
v0.1/wire.lock   ex:t/api.alpha = 1 method u32    ex:t/api.beta = 2 method
v0.2/wire.lock   ex:t/api.alpha = 2 method u32    ex:t/api.beta = 3 method
witforge diff wit/v0.1 wit/v0.2 --check    no contract changes … COMPATIBLE, exit 0
witforge proto wit                         `ex:t/api.alpha` was renumbered from 1 to 2, exit 1
```

### Names that do not compile (B1, B9)

```text
enum x-kind { foo }          glue ::Foo            prost TypesXKind::TypesXkindFoo
record r { foo-BAR: u32 }    glue foo_BAR          prost foo_bar
enum e { HTTP, other }       glue ::HTTP           prost ::Http
```

### Lockfile lost through a pipe (C2)

```text
witforge lock wit/v0.1 --first | head -1     400 members: no wire.lock in 30 of 30 runs
witforge lock wit/v0.1 --first > /dev/full   exit 101, no wire.lock
```

### Action input names (E1)

```text
action/index.js   INPUT_${name.replaceAll('-', '_').toUpperCase()}     reads INPUT_DIFF_OLD
@actions/core     INPUT_${name.replace(/ /g, '_').toUpperCase()}       runner sets INPUT_DIFF-OLD
```

## Corrections to the first summary

A summary of this audit was given in conversation before the findings were re-verified. These
statements in it were wrong or too broad, and the tables above supersede them:

- "About 60 defects": the tables hold 70 findings, and many rows group more than one defect.
- "Twenty lines of WIT produce hundreds of megabytes or a stack overflow" joined two cases. The
  size growth needs about twenty lines; the stack overflow needs an alias chain thousands of
  lines long.
- "`diff` fails for any record field typed by a dependency alias": only an alias of a named
  dependency type fails; aliases of primitives and lists work (A15).
- "SARIF has no file locations": drift diagnostics carry paths. Contract diagnostics carry a
  logical name in the path field, and nothing carries a line (C6).
- "The fuzzers never reach the emitters" was inferred from the local corpora of the recorded
  short runs; a long run was not attempted (E8).
- "Released directories can never be removed" is a rule of `ci/check-release-freeze.sh`, not of
  the tool (D1).
- The proposal to put the dependency version into type identity is withdrawn: it would change
  frozen lockfiles. The recorded fix refuses two versions of one package (A3).
- A changed dependency alias was presented as undetected. `check --against` misses it, but the
  generators refuse it through the pinned-dependency-graph rule (A8).
- The lockfile loss through `head` depends on output size; it does not reproduce with a
  six-member contract (C2).
- Non-determinism across machines for multi-file contracts is inferred from how the file system
  orders directory entries; only the dependence on enumeration order was demonstrated (C1).
- The byte-list fix and the multi-file ordering fix change generated output for affected
  contracts. They are corrections that need a migration note, not transparent fixes.
- The five "retry CI" commits were attributed to cancelled pending runs. Their messages name the
  macOS runner queue; the cause was not determined (E7).

## Recorded, not scheduled

- The four items the first audit left unscheduled (module splits, an in-memory loading API for
  the fuzz target, duplicate dev-dependency versions, mutation testing) stay unscheduled, except
  that the in-memory loading API is now part of `W10-096`.
- wit-parser is pinned to `=0.263.0`. The cost and cadence of moving the pin, and what a WIT
  syntax change does to released contracts, were not examined.
- A rule observed while verifying A8: every version in a WIT root must use an identical
  dependency graph. How a dependency is updated under that rule is not documented; `W10-051`
  asks for the documentation and does not question the rule.

## Not examined

- Windows and macOS: every probe ran on Linux.
- The Action on a hosted runner, and the release workflow end to end.
- Decoding generated protobuf with non-Rust generators (`W11-034`).
- Performance, other than the pathological inputs in A17.

## Priorities

The order below is a recommendation; the dependency edges in the plan enforce only what is
marked.

1. **Unblock and correct what is published** (`W10-039` to `W10-043`). When the audit was
   written the Action's `diff` gate did not run, the next release could not pass CI, and the
   quickstart failed at its first command. These are small and independent. The owner decided on
   2026-11-18 that they ship with 0.6.0 and not as a 0.5.1 patch release, so `W10-044` is dropped
   and they block `W10-069`.
2. **Restore the wire-identity guarantee** (`W10-045` to `W10-058`). Start with A1 to A3: each is
   a silent wire change that today passes every gate.
3. **Make "generation succeeded" mean "it compiles"** (`W10-059` to `W10-068`). Build the
   compile harness (`W10-068`) early, because it is what proves the other fixes and what keeps
   the class from returning.
4. **Stop the tool losing work** (`W10-073`, `W10-075`, `W10-077`, `W10-078`).
5. **Release 0.6.0** (`W10-069`, blocked by everything in 1 to 4) with a migration section,
   because the fixes change generated output or refuse contracts that were accepted. On
   2026-11-19 the owner added `W10-070`, `W10-072`, and `W10-074` to this release, so that
   consumers regenerate once.
6. **Implement what 1.0 freezes** (`W10-093`, `W10-095`, `W10-070`, `W10-059`, `W10-098`). Each is
   a surface that cannot change after 1.0 without a major version. The owner decided each on
   2026-11-19, and the task records the decision.
7. Everything else in the cut: diagnostics, CLI workflow, release integrity, adopter
   documentation.

Decisions made after this record was written are in the tasks' `Decision` sections, which say
whether the owner made them or whether they are a default chosen while the task was documented
([`aplan/README.md`](../../aplan/README.md), Task bodies).

`W10-001` remains parked until 2027-01-15. `W10-002` (release 1.0.0) now also waits for
`W10-069` and `W10-098`.
