# Independent adopter evaluation and outreach protocol

Status: adopted 2026-11-11 by [`W11-004`](../../aplan/tasks/W11-004.md). This document defines how witforge evaluates and contacts independently governed candidate consumers. Aplan remains the source of task status.

## Decision

Candidate evaluation uses **hard eligibility gates followed by categorical ratings**. A project is contacted only after the gates establish an objective current problem and a bounded useful adoption slice. Ratings are `strong`, `mixed`, `weak`, or `unknown`; they are not converted into a numerical score.

Research may produce an evidence packet, source links, and a question outline. The human owner writes and posts every external message in their own words. This is the default for all projects, not only projects whose contribution policy restricts generated communication.

This protocol protects three boundaries:

- architectural resemblance is not evidence that a project needs witforge;
- a missing runtime, lifecycle, or application policy is not a witforge compiler gap;
- an example, fork, unmerged experiment, or project governed by the same maintainer is not independent adoption for [`W10-001`](../../aplan/tasks/W10-001.md).

The upstream sources and channels below were reviewed on 2026-11-11. Every candidate investigation must pin the reviewed upstream commit or release, and every outreach task must recheck the project's current policy and channel on the day of contact.

## Evidence standard

### Source requirements

A report must cite primary evidence for every material claim:

1. upstream source, generated files, protocol definitions, tests, release notes, architecture decisions, or maintained documentation at a pinned commit or release;
2. a public maintainer statement in the project's canonical channel;
3. observed output from a reproducible disposable compatibility exercise, including exact witforge and upstream revisions.

Search results, third-party summaries, repository popularity, and analogous projects may locate evidence but cannot establish a claim. A branch, proposal, roadmap item, or stale document must be labelled as such and cannot be described as shipped behavior.

Each claim records:

- the claim in falsifiable terms;
- its primary URL or repository path and pinned revision;
- whether it is `observed`, `maintainer-stated`, or a `hypothesis`;
- the observation date;
- contrary evidence and unresolved uncertainty.

An inference remains a hypothesis even when it appears likely. Silence is not confirmation.

### Claims that require proof

| Claim | Minimum evidence |
|---|---|
| Current use case | Maintained code or a shipped/currently supported contract exercises the path. A future plan alone is insufficient. |
| Duplicated contract source | At least two separately edited definitions represent the same semantic values or operations, with concrete adapters, code generation, or synchronization work between them. Multiple generated views of one source are not duplication. |
| Compatibility problem | A documented compatibility obligation, historical fixture, migration rule, drift defect, frozen protocol, or reproducible cross-version failure exists. The mere presence of version numbers is insufficient. |
| Wire or schema need | A maintained consumer depends on stable field/method identity, machine-readable value schemas, or a second transport. “Protobuf may be useful” is insufficient. |
| Adoption blocker | The smallest useful slice fails a pinned witforge check or emitter exercise, or a required semantic capability is absent from witforge's documented contract. The report names the exact construct and affected production path. |
| Migration cost | The report inventories definitions, adapters, generated artifacts, tests, release policy, and compatibility commitments that would change; line count or intuition alone is insufficient. |
| Maintainer interest | A public response from a maintainer or authorized project representative confirms the stated problem or welcomes a bounded experiment. A reaction, view count, or third-party comment is insufficient. |
| Successful independent adoption | An independently governed project accepts and uses a supported witforge release on a maintained mainline or release path, with generated-output/staleness checks and consumer-visible tests recorded. |

### Capability classification

Every observed problem belongs to exactly one primary class:

1. **Solved now by witforge.** The supported release already supplies the required WIT checking, locked protobuf identity, plain Rust/Serde types and traits, protobuf messages and glue, JSON Schema, compatibility diff, dependency projection, or adjacent-version conversion.
2. **Upstream contract redesign.** Adoption would require the project to change deliberate domain semantics, split or flatten its API, replace a working ABI, or add a second transport only to fit witforge. This is an upstream tradeoff, not evidence of a witforge defect.
3. **Bounded witforge gap.** The need fits witforge's runtime-independent contract mission, is required by a named maintained consumer, and can be specified and tested without absorbing application or runtime policy. Implementation requires a separate aplan task with an observable regression test.
4. **Runtime or application concern.** Component execution, Canonical ABI bindings, process framing, async scheduling, cancellation, callbacks, capability grants, resource limits, discovery, packaging, signing, retries, and failure policy remain with wit-bindgen, Wasmtime, another runtime, or the application.
5. **No demonstrated problem.** The project is useful prior art, but no present consumer benefit has been established.

A report may list secondary classes, but it must not relabel an upstream redesign or runtime concern as a witforge feature gap.

## Eligibility gates

All four gates must pass before maintainer outreach.

| Gate | Pass condition | Failure result |
|---|---|---|
| G1 — current problem | Primary evidence shows a maintained current use case plus duplicated maintenance, compatibility pressure, or a real wire/schema need. | `no-contact`, or `defer` when a dated upstream trigger could make the use case real. |
| G2 — relevant capability | Witforge solves a material part now, or one bounded mission-compatible gap blocks it. | `no-contact` when the work is only upstream redesign, runtime machinery, or speculative parity. |
| G3 — useful slice | A smallest production-relevant contract, execution path, generated output, and success check can be named without a wholesale rewrite. | `no-contact` when only a platform rewrite creates value; `defer` when a planned seam is not implemented yet. |
| G4 — ownership boundary | The project retains domain semantics and runtime policy; witforge remains a deterministic contract compiler. | `no-contact` when value depends on witforge owning execution, lifecycle, security policy, or distribution. |

`Unknown` does not pass a gate. Investigate it or defer with a concrete revisit trigger. A famous project, a strong WIT resemblance, or a high categorical rating cannot override a failed gate.

### Outcomes

- **Contact:** all gates pass, the evidence packet contains project-specific falsifiable questions, and the canonical channel permits the approach.
- **Defer:** evidence is unavailable or the useful seam is explicitly planned but not current. Record the missing fact, upstream trigger, owner, and earliest review date.
- **No-contact:** no objective benefit or bounded slice is demonstrated, the proposal would displace a working design for witforge's benefit, or only out-of-scope runtime work remains. Closing an outreach task with a cited no-contact decision is preferable to sending a generic pitch.

## Common fit matrix

Complete this matrix only after the eligibility gates. Ratings support comparison; they do not add up to a score.

| Dimension | `strong` | `mixed` | `weak` | `unknown` |
|---|---|---|---|---|
| Contract shape | The useful slice is value-oriented, synchronous, and inside the supported subset. | A useful subset exists beside unsupported constructs or requires a small justified remodel. | Core semantics depend on resources, recursion, maps, streams, futures, callbacks, or dynamic values. | The authoritative contract cannot be located or its status is unclear. |
| Execution paths | Two or more maintained paths share one semantic contract and need generated projections or conformance. | Multiple paths exist but intentionally differ, or only part of the contract is shared. | There is one path, or adding another path would exist only to demonstrate witforge. | Implemented paths and ownership are unclear. |
| Wire/schema need | Stable protobuf identity, schema consumers, or generated cross-transport values are current requirements. | A real consumer exists but stable wire/schema value is partial or prospective. | No maintained wire or schema consumer exists. | The downstream contract cannot be verified. |
| Evolution pressure | Releases, frozen fixtures, adapters, or compatibility incidents demonstrate active evolution cost. | The API evolves, but compatibility obligations or drift costs are limited. | The contract is disposable, private, or deliberately hard-cut without compatibility needs. | Release and compatibility policy is undocumented. |
| Migration cost | A bounded slice can adopt incrementally while preserving behavior and existing runtime choices. | Adoption needs notable adapter, API, or release work but has a reversible path. | It requires a platform rewrite, breaks supported consumers, or duplicates mature tooling. | Required changes and affected consumers are not inventoried. |
| Smallest useful slice | One maintained path, contract boundary, output set, and end-to-end success check are explicit. | A plausible slice exists but one material boundary awaits maintainer confirmation. | Only a broad framework or whole-protocol replacement creates value. | No production-relevant slice has been identified. |

Every rating includes one sentence of evidence and a primary citation. `Mixed` and `unknown` identify the exact boundary or missing fact.

## Candidate report contract

Each file under `docs/design/adopters/` uses the following structure:

1. **Snapshot:** upstream repository, pinned commit/release, review date, maintenance/release status, and primary sources.
2. **Current architecture:** authoritative contracts, hand-written and generated projections, execution paths, adapters, compatibility policy, and tests.
3. **Problem evidence:** claims using the evidence standard; hypotheses are visibly separate.
4. **Classification:** what witforge solves now, upstream redesigns, bounded witforge gaps, runtime/application concerns, and non-problems.
5. **Eligibility gates:** pass/fail/defer with citations.
6. **Fit matrix:** all six categorical ratings with reasons.
7. **Smallest useful slice:** WIT ownership, generated outputs, retained runtime adapters, migration steps, behavioral checks, and rollback.
8. **Disposable exercise:** exact revisions and commands used to check the slice; scratch output is not adoption evidence.
9. **Recommendation:** contact, defer with trigger, or no-contact.
10. **Outreach evidence:** current policy/channel, evidence packet, human-authored questions, URL/date, and outcome.

The report must say what result would falsify its recommendation.

## Outreach protocol

### Before contact

The task owner must:

1. finish the candidate report and pass all four eligibility gates;
2. re-read the current contribution and communication policy;
3. search issues, discussions, RFCs, and public chat history for an existing thread;
4. prefer an existing canonical thread over opening a duplicate;
5. verify that the message asks about a maintained project problem rather than advertising witforge;
6. avoid implementation work or a pull request until maintainers express interest.

Private email and unsolicited direct messages are not canonical outreach unless the project explicitly requests them.

### Evidence packet and human message

The research artifact may contain:

- links to the exact upstream definitions and adapters;
- a two- or three-sentence statement of the observed maintenance or compatibility problem;
- the smallest proposed adoption slice;
- capabilities witforge supplies today;
- known unsupported constructs and runtime non-goals;
- two to four falsifiable questions;
- the exact experiment offered if maintainers confirm interest.

It must not contain ready-to-post prose. The human owner writes the final message, keeps it concise, checks every claim, and posts it under their own identity. The message must make rejection or correction easy. It must not imply endorsement, promise unplanned features, compare projects competitively, or ask maintainers to design witforge.

Good questions have observable answers, for example:

- “Are these two definitions intentionally independent, or is their drift a maintenance problem?”
- “Would generated plain Rust and protobuf values for this bounded operation be useful while the current runtime remains unchanged?”
- “Does this compatibility rule require preserving the existing field numbers?”
- “Is a focused experiment on this path welcome, or should we record it as a non-goal?”

“Would a general WIT plugin framework be useful?” is too broad to send.

### Follow-up and recording

Record the contact date, canonical URL or public channel, project policy source, exact questions in the human-authored message, and the responding maintainer's role. Do not bump an unanswered thread unless project policy or a maintainer invites follow-up.

Thirty calendar days after the initial public contact:

- a substantive response is summarized and linked, including corrections and rejection;
- an explicit rejection is recorded as a valid research outcome and does not become a witforge backlog item;
- no response is recorded as `no-response` with the initial date and 30-day decision date;
- a no-contact decision records the failed gate and evidence instead of inventing a URL.

Rejection, no-contact, and no-response may complete the corresponding research/outreach task, but none weakens or satisfies `W10-001`. Later unsolicited interest reopens evaluation from current evidence; it does not retroactively change the recorded outcome.

## Canonical channel registry

These entries cover the eleven independently governed candidates scheduled by `W11-006` through `W11-027`. Same-maintainer projects alint and arouter, and aplan's explicit plugin non-fit, are not external-outreach candidates under this protocol.

| Candidate | First contact | Policy and source reviewed 2026-11-11 | Required handling |
|---|---|---|---|
| [Language Check](https://github.com/KaiErikNiermann/LangCheck) | Search and open a focused GitHub issue. | Its [contribution guide](https://github.com/KaiErikNiermann/LangCheck/blob/main/CONTRIBUTING.md) directs reports to GitHub Issues and requires focused, tested changes. | Ask about the observed checker-contract duplication and a bounded integration before submitting code. |
| [lla](https://github.com/chaqchase/lla) | [GitHub Ideas discussion](https://github.com/chaqchase/lla/discussions/categories/ideas). | The repository enables Discussions and publishes an Ideas category; its [community profile](https://api.github.com/repos/chaqchase/lla/community/profile) exposes no contribution guide. | Recheck the category and repository policy; do not use an implementation PR as design discovery. |
| [nginx-lint](https://github.com/walf443/nginx-lint) | Search and open a focused GitHub issue only if a value-only seam passes the gates. | Repository [metadata](https://api.github.com/repos/walf443/nginx-lint) shows Issues enabled and Discussions disabled; its [community profile](https://api.github.com/repos/walf443/nginx-lint/community/profile) exposes no contribution guide. | Record no-contact rather than asking maintainers to remodel a resource-heavy API solely for witforge. |
| [loadr](https://github.com/levantar-ai/loadr) | Search and open a focused GitHub issue only if a current shared contract passes the gates. | Repository [metadata](https://api.github.com/repos/levantar-ai/loadr) shows Issues enabled and Discussions disabled; its [community profile](https://api.github.com/repos/levantar-ai/loadr/community/profile) exposes no contribution guide. | Disclose experimental status and distinguish shipped behavior from planned documentation. |
| [Pact plugins](https://github.com/pact-foundation/pact-plugins) | Comment on [Wasm-plugin Discussion #85](https://github.com/pact-foundation/pact-plugins/discussions/85), unless closed or superseded. | Discussion #85 is the maintainer-created channel for the draft Wasm proposal; its maintainer reply also suggests an RFC may supersede it. | Recheck proposal status and ownership before commenting; open a new Discussion only if the existing thread is no longer canonical. |
| [Conduit](https://github.com/ConduitIO/conduit) | Search and open a GitHub Discussion for design discovery. | The [contribution guide](https://github.com/ConduitIO/conduit/blob/main/CONTRIBUTING.md) directs questions to Discussions and feature requests to Issues. | Use a Discussion for the initial falsifiable questions; create an issue or implementation only when maintainers direct it. Recheck the current Component Model ADR status. |
| [dprint](https://github.com/dprint/dprint) | Search and open a focused GitHub issue only after a bounded current need passes the gates. | Repository [metadata](https://api.github.com/repos/dprint/dprint) shows Issues enabled and Discussions disabled; its [community profile](https://api.github.com/repos/dprint/dprint/community/profile) exposes no contribution guide. | Acknowledge the mature custom protocols; do not pitch replacement absent a maintained next-version need. |
| [Zed](https://github.com/zed-industries/zed) | GitHub Discussion, never an unsolicited issue or PR for this proposal. | Zed's [contribution guide](https://github.com/zed-industries/zed/blob/main/CONTRIBUTING.md) requires prior Discussion for Extension API changes and rejects autonomous-agent communication. | The human owner writes the message independently; research supplies facts and questions only. Explicitly allow a no-current-benefit conclusion. |
| [Zellij](https://github.com/zellij-org/zellij) | Ask first in the public [Discord](https://discord.gg/MHV3n76PDq) or [Matrix](https://matrix.to/#/#zellij_general:matrix.org) general channel. | Zellij's [contribution guide](https://github.com/zellij-org/zellij/blob/main/CONTRIBUTING.md) asks contributors to check large work with maintainers and to avoid LLM-generated issue or PR descriptions. | The human owner writes the outreach. Open a durable plugin-system issue only if maintainers request one. |
| [Nushell](https://github.com/nushell/nushell) | Prefer a searched GitHub Discussion for durable design discovery; follow maintainer direction to Discord or an issue. | Nushell's [contribution guide](https://github.com/nushell/nushell/blob/main/CONTRIBUTING.md) invites Discord, GitHub Discussions, or draft PRs for early help and requires core-team discussion before major changes. | Do not propose implementation before core-team interest; keep the question to bounded message or compatibility needs. |
| [Lix](https://github.com/opral/lix) | Ask first in the public [Lix Discord](https://discord.gg/gdMPPWy57R). | The [project site](https://lix.dev/) identifies Discord as its community/help channel; GitHub Discussions are disabled, and the [contribution guide](https://github.com/opral/lix/blob/main/CONTRIBUTING.md) covers code submission but not design outreach. | Distinguish lessons for witforge from benefits to Lix. Open a GitHub issue only if maintainers request durable tracking. |

## Independent-adoption accounting

A candidate counts toward `W10-001` only when all of the following are true:

- governance and acceptance are independent of witforge's maintainer;
- a maintained upstream mainline or release path consumes a supported witforge release;
- the exact WIT contract, generated outputs, witforge version, and stale-output check are recorded;
- upstream tests exercise consumer-visible semantics rather than merely compiling generated files;
- the independent maintainer accepts the integration and keeps it in a supported path.

The following never count by themselves:

- maintainer interest, design approval, or an unanswered proposal;
- a local scratch run, fork, draft PR, unmerged branch, or sample repository;
- alint, arouter, another same-maintainer product, or a witforge-owned framework;
- documentation, stars, downloads, or a project's use of WIT without witforge;
- a project that must be reshaped only to manufacture an adopter.

If a real adopter exposes a bounded witforge defect, create and complete a separate tested task. If no candidate qualifies, keep `W10-001` open and continue supported 0.4.x corrections rather than redefining independent adoption.