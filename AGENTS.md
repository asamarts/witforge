# AGENTS.md: witforge

witforge is a WIT-first contract compiler. The hand-written WIT package is the
source of truth for a contract; witforge checks it, locks its wire numbers, and
generates plain Rust types/traits and version adapters, a protobuf schema,
JSON Schema, and protobuf glue.

## Current state

- The latest release is the top dated section of [`CHANGELOG.md`](CHANGELOG.md).
  `main` can be ahead of it: the `[Unreleased]` section lists what no release
  has yet.
- Implemented: package loading with standard `deps/` dependencies and `deps sync`,
  the restricted-subset checker (including `char`, fixed-length `list<T, N>`, and
  `map<K, V>`), the append-only lockfile (v2, with declaration-order
  fingerprints), the Rust emitter (opt-in Serde derives, opt-in `no_std` +
  `alloc`, opt-in float-free `Eq`/`Hash` derives, `convert.rs` version adapters
  with opt-in `TryFrom` impls, `worlds.rs`), the protobuf, JSON Schema (Draft 7
  default, opt-in Draft 2020-12), glue, and Markdown documentation (`doc`)
  emitters, versioned JSON contract IR (`inspect`), contract `diff` (including
  `--git-base <REV>`), release-freeze enforcement, structured diagnostics
  (`--format human|json|sarif`), `witforge.toml` projects (including
  `[project.<name>.policy]`), the optional `witforge-build` helper crate, static
  `musl` Linux release archives, the GitHub Action, and the Astro + Starlight
  documentation and promotional site under `site/` (deployed via
  `.github/workflows/docs.yml`).
- The commands are `witforge check`, `lock`, `rust`, `proto`, `schema`, `glue`,
  `doc`, `generate`, `init`, `new-version`, `deps sync`, `release`, `retire`,
  `diff`, `inspect`, `freeze`, `completions`, and `explain`.
- The normative specifications are [`docs/design/model.md`](docs/design/model.md),
  [`docs/design/lockfile.md`](docs/design/lockfile.md), and
  [`docs/design/emitters.md`](docs/design/emitters.md) (Rust, protobuf, glue,
  conversions, JSON Schema, and `witforge.toml`). `diff`, diagnostics,
  dependencies, and the Action are specified in
  [`docs/design/diff.md`](docs/design/diff.md),
  [`docs/design/diagnostics.md`](docs/design/diagnostics.md),
  [`docs/design/dependencies.md`](docs/design/dependencies.md), and
  [`docs/design/action.md`](docs/design/action.md).
- The narrative sequence is [`ROADMAP.md`](ROADMAP.md).
- The 1.0 public API policy is [`docs/design/public-api.md`](docs/design/public-api.md), the
  conformance matrix is [`docs/design/conformance.md`](docs/design/conformance.md), and the 1.0
  release criteria and deprecation runways are [`docs/design/release-1.0.md`](docs/design/release-1.0.md).
- The task-level source of truth is [`aplan/`](aplan/README.md).

## Planning and status: aplan

Planning, tasking, dependencies, and closure evidence live under `aplan/tasks/`.
They are not in issues, TODO comments, chat, or prose. The plan is validated by
`aplan check` and `aplan verify`.

- Task IDs follow `W<phase>-<nnn>`; cuts are `M-p<phase>` with label `p<phase>`.
- Every non-milestone task belongs to exactly one cut.
- Code tasks close on a named `cargo test` citation. A task carrying `untestable`
  is a claim that no test could fail if the work were reverted, and its body says
  why. Milestones are exempt because a cut is proved by its members.
- `deny_warnings` is on: every `aplan` finding blocks, including warnings.
- If work does not fit a task, write the task first with
  `aplan add --from` or the MCP `aplan_add` tool; do not start with an untracked
  change.
- Do not hand-write `status: done` or edit `evidence`. Use `aplan close` (or the
  MCP `aplan_close` tool) so the evidence gate runs. Use `aplan start/stop` rather
  than editing `status`.
- Before committing plan or agent-contract changes:

  ```sh
  aplan check
  aplan verify
  aplan export-agents-md --check AGENTS.md
  ```

- Useful commands: `aplan ready`, `aplan show <id>`, `aplan why <id>`,
  `aplan roadmap`, `aplan summary`, `aplan critical-path`.
- If `aplan` MCP tools are available, prefer them over shelling out; they go
  through the same evidence and validation gates.
- `AGENTS.md` carries a generated aplan block between the `aplan:begin` and
  `aplan:end` markers. Edit the plan or `.aplan.yml`, then regenerate with
  `aplan export-agents-md --write AGENTS.md`; do not hand-edit inside the
  markers. `CLAUDE.md` is a symlink to this file.
- If a done task’s traced section or mutable file evidence changes after
  re-review, run `aplan close <id> --restamp`; never edit the fingerprints by
  hand. Closing or changing a task can also make the generated agent block
  stale; regenerate it and rerun `aplan export-agents-md --check AGENTS.md`.

## Reporting aplan issues, friction, and feature requests

Report `aplan` bugs, workflow friction, documentation gaps, and feature requests as GitHub issues on the public [`asamarts/aplan`](https://github.com/asamarts/aplan/issues) repository (do not write local pilot logs or add tasks directly in `~/projects/aplan`):

1. Search first: `gh issue list -R asamarts/aplan --state all --search "<words>"`. If an issue already exists, add new reproduction details there.
2. Reproduce on a minimal generic scratch plan under `~/.tmp`.
3. File with exact command, exit code, output, and `aplan --version` (plus `aplan facts --identity` when relevant): `gh issue create -R asamarts/aplan --title "..." --label bug|enhancement --body-file <file>`.
4. **Keep public issues minimal and generic:** reproduce on a generic scratch plan under `~/.tmp`
   rather than attaching repository-specific task files or local host paths.
5. **Do not edit `~/projects/aplan` directly** as a workaround from here.

## Build and test

```sh
mkdir -p "$HOME/.tmp"
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p witforge-build --all-targets --all-features -- -D warnings
TMPDIR="$HOME/.tmp" cargo test --workspace --locked
TMPDIR="$HOME/.tmp" cargo test -p witforge-build --all-features --locked
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps
TMPDIR="$HOME/.tmp" cargo +1.88 test --workspace --locked   # MSRV
TMPDIR="$HOME/.tmp" cargo +1.88 test -p witforge-build --all-features --locked
TMPDIR="$HOME/.tmp" npm --prefix site run verify            # site tests, check, build, links
```

- The tests create their scratch directories under `std::env::temp_dir()`, which
  is `/tmp` unless `TMPDIR` says otherwise. Set `TMPDIR` as above for every test
  run on this host.
- Do not change cargo configuration, parallelism, or host-wide settings to make a
  build pass.
- Put scratch worktrees, target dirs, and large downloads under `~/.tmp`, not
  `/tmp`.
- CI runs on GitHub-hosted runners. Never attach a self-hosted runner to this
  public repository (see `ci/README.md`).

## Documentation and contracts

- Design decisions live in `docs/design/overview.md` and the linked specs.
- User-facing guides live under `docs/` and are indexed by `docs/SUMMARY.md`;
  `site/scripts/sync-docs.mjs` syncs them into Starlight and generates
  `llms.txt` / `llms-full.txt`. Legacy `.html` and `/guide/` compatibility
  redirects are declared in `site/scripts/redirects.json`.
- Published `docs/` guides, `README.md`, and `site/` pages must contain natural
  human prose with zero em dashes (checked by `site/scripts/check-links.mjs` and
  `crates/witforge-cli/tests/docs.rs`).
- Generated output must be deterministic, LF-only, and checked with `--check`.
- A contract change is a new version directory, not an in-place edit to a
  released one. See `docs/design/lockfile.md` and `docs/design/overview.md`.
- Do not add a second task list or status copy to the README or roadmap; link to
  the plan instead.

<!-- aplan:begin -- generated by `aplan export-agents-md`; edits between these markers are lost -->
<!-- written by aplan 0.7.0, surface d78e626e4300 -->

## The plan is the source of truth

Planning, tasking and status live under `aplan/tasks/`, and what judges them is `aplan check`
followed by `aplan verify`. Not in issues, not in a TODO comment, not in prose: a document that
carries status becomes a second copy of it, and the second copy drifts.

Task ids follow this plan's own conventions (`W1-nnn`, `W10-nnn`, `W11-nnn`, `W12-nnn`,
`W2-nnn`, `W3-nnn`, `W4-nnn`, `W5-nnn`, `W6-nnn`, `W7-nnn`, `W8-nnn` or `W9-nnn`). If work does
not fit an existing task, **write the task first**, then do the work: a change with no task is
a change nobody can find later.

## The loop

```sh
aplan ready                 # what can be started now: blockers all done, in a derived order
aplan why W1-nnn            # why something cannot be started yet
# ... implement, commit ...
aplan close W1-nnn --evidence commit:COMMIT_SHA --evidence test:REGISTERED_TEST_NAME
aplan check && aplan verify # what CI will run
```

**A status is a claim, and `close` requires proof of it.** Evidence is a citation something
else resolves: a commit reachable from `HEAD` whose diff touched this task's declared `paths`,
or a test a runner can list and run. There is no "mark it done". If a task cannot be closed
honestly, it is not done, and saying so beats finding a weaker citation.

## Reusing a plan shape

Reach for a repository-supplied blueprint when task fields or sibling topology recur. A
blueprint is one explicit local plan-authoring file, not a catalogue entry or an executable
program. It can still propose a complete task graph, so inspect the selected file and preview
its whole effect before applying it.

```sh
aplan blueprint apply --file FILE_PATH --cut CUT_ID --set NAME=VALUE --dry-run
```

Repeat `--set` exactly once for each declared parameter, and omit it when none are declared.
Inside the file, `local:key` names a new sibling and `task:id` names an existing task. Review
the reported source path and digest, predicted tasks, ids, files and findings; apply by
rerunning the same command without `--dry-run`.

## What closes a task here

- A `done` task must cite `test` evidence (`evidence.require_min_strength`).
- Tasks tagged `untestable` or `milestone` are exempt, and a task claiming the exemption should
  say in its body why nothing stronger exists.
- Every task that is not a milestone declares `paths`, and a `done` one without them is reported
  (`APLAN023`). That is what makes a `commit` citation checkable: `verify` refuses one whose diff
  touched nothing the task claims, and a task without `paths` has quietly weakened its own
  evidence to "`HEAD` reaches some commit".
- Every live task that is not a milestone carries `effort_days`; one without it is reported
  (`APLAN009`).
- Every task that is not a milestone belongs to exactly one cut, or is tagged `cross-cutting` out
  loud (`APLAN022`). Put a new task in its milestone's `blocked_by` when you write it.
- Closing a task stamps the `traces` sections it names, and a later edit to one of them reports
  the task (`APLAN018`). Re-read the section, then `aplan close --restamp` if it still holds.
- A `test` citation resolves against the configured runners (`witforge`, `witforge-build`,
  `witforge-cli`): it has to be a name the runner lists, and where several runners list it the
  citation is refused rather than guessed (`APLAN013`).
- **Every warning blocks** here (`deny_warnings`): a finding is a finding.

## Through MCP

`aplan mcp` serves these tools, and they go through the same gates as the CLI, so they refuse
what it refuses. Prefer them over shelling out.

- `aplan_identity` -- Name the build of this server: its version, the commit it was built from,
  and the digests of its capability document and its own image.
- `aplan_ready` -- List tasks whose blockers are all done, so an agent can pick real work.
- `aplan_why` -- Explain what is blocking a task: its own blockers, and any unfinished cut,
  without expanding that cut's membership.
- `aplan_check` -- Validate the plan and return its diagnostics, and `denied`: whether this plan
  fails the build under the repository's warning policy.
- `aplan_verify` -- Resolve the plan's evidence against the repository (L1/L2): a cited commit
  must be reachable from HEAD and touch the task's `paths`, a cited file must exist.
- `aplan_add` -- Create a task with an explicit id and phase, or mint the next id in this plan's
  own convention (including its configured ULID strategy) and derive the phase from its cut.
- `aplan_show` -- One task whole: its `paths`, `traces` and `evidence`, which no other tool
  returns, plus what the graph resolves (its cut, whether it is ready, what it waits on and what
  it releases) and every finding attached to it.
- `aplan_dep` -- Add or remove one dependency edge: `after` is the task that must finish first.
- `aplan_close` -- Close a task with evidence, resolved at L2 before it is written: a cited
  commit must be reachable from HEAD and touch the task's `paths`, a cited file must exist, and a
  cited test must be one a configured runner lists.
- `aplan_claim` -- Record that you are working on a task, so other agents sharing this Git
  repository and all of its linked worktrees can see the same local ephemeral claim.
- `aplan_release` -- Drop a claim, reporting whose it was.
- `aplan_start` -- Record that work on a task has begun, setting `in_progress`.
- `aplan_stop` -- Record that work on a task has stopped, returning it to `todo`.

<!-- aplan:end -->
