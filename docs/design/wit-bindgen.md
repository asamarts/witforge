# wit-bindgen and witforge: boundary, overlap, and lessons

Status: architecture decision and comparative audit, 2026-11-11. Owned by [`W11-005`](../../aplan/tasks/W11-005.md).

## Decision

`wit-bindgen` and witforge are complementary WIT consumers, not competing implementations.

- [`wit-bindgen`](https://github.com/bytecodealliance/wit-bindgen) generates guest-language bindings that implement Component Model imports, exports, and Canonical ABI lift/lower. Its Rust `generate!` macro and generated `export!` macro participate in the component binary ABI.
- witforge generates runtime-independent contract artifacts: stable owned Rust values and object-safe traits, locked protobuf identities and conversion glue, JSON Schema, compatibility reports, and adjacent-version adapters.
- Wasmtime's [`component::bindgen!`](https://docs.wasmtime.dev/api/wasmtime/component/macro.bindgen.html) separately generates typed **host** loading, linking, and call bindings.

Witforge must not generate Canonical ABI shims, guest export machinery, Wasmtime host bindings, component loading, or runtime policy. It should use `wit-bindgen` and Wasmtime as the integration points for Component Model execution.

One useful bridge already exists without new witforge code: guest Rust `wit_bindgen::generate!` can remap individual WIT types through `with` to witforge-generated owned types. A disposable compile exercise proved this for a record and a payload variant, including generated `export!` lowering. This avoids a second application-level conversion and any allocation that conversion would require. It does not remove the Canonical ABI's required lift/lower work, and Wasmtime host bindings do not offer the same arbitrary value-type remapping.

No bridge emitter, runtime dependency, or parity task is justified now. A real adopter and measurements are the trigger.

## Reviewed evidence

The audit used primary sources at these revisions:

| Artifact | Pin | Relevant evidence |
|---|---|---|
| wit-bindgen release | [`v0.63.0`](https://github.com/bytecodealliance/wit-bindgen/releases/tag/v0.63.0), commit [`29e0393`](https://github.com/bytecodealliance/wit-bindgen/commit/29e03934154913417189a9bfb8e2fa6682d1d795), 2026-10-14 | Latest published release, binaries, crate version, MSRV 1.88 |
| wit-bindgen current source | commit [`a3533a2`](https://github.com/bytecodealliance/wit-bindgen/commit/a3533a263d455edbfcadc512a9ce11c17fce5387), 2026-11-10 | Current generator architecture, options, tests, and unreleased changes |
| Project scope | [README](https://github.com/bytecodealliance/wit-bindgen/blob/a3533a263d455edbfcadc512a9ce11c17fce5387/README.md) | Guest focus; host execution explicitly outside the repository |
| Core generator | [`WorldGenerator`](https://github.com/bytecodealliance/wit-bindgen/blob/a3533a263d455edbfcadc512a9ce11c17fce5387/crates/core/src/lib.rs) and [`Files`](https://github.com/bytecodealliance/wit-bindgen/blob/a3533a263d455edbfcadc512a9ce11c17fce5387/crates/core/src/source.rs) | World traversal phases, exposed `wit-parser` model, deterministic file collection |
| Rust generator | [`Opts`](https://github.com/bytecodealliance/wit-bindgen/blob/a3533a263d455edbfcadc512a9ce11c17fce5387/crates/rust/src/lib.rs) | Ownership modes, remapping, selection, attributes, stubs, async, maps, and runtime configuration |
| Rust macro | [`generate!` parser](https://github.com/bytecodealliance/wit-bindgen/blob/a3533a263d455edbfcadc512a9ce11c17fce5387/crates/guest-rust/macro/src/lib.rs) and [published macro documentation](https://docs.rs/wit-bindgen/0.63.0/wit_bindgen/macro.generate.html) | Package/world selection, dependency paths, source-mapped diagnostics, feature selection, deterministic remapping |
| Test architecture | [tests README](https://github.com/bytecodealliance/wit-bindgen/blob/a3533a263d455edbfcadc512a9ce11c17fce5387/tests/README.md) and [CI](https://github.com/bytecodealliance/wit-bindgen/blob/a3533a263d455edbfcadc512a9ce11c17fce5387/.github/workflows/main.yml) | Cross-language codegen compilation, actual component runtime tests, Miri, feature combinations, MSRV and platforms |
| Wasmtime host bindings | [`wasmtime::component::bindgen!`](https://docs.wasmtime.dev/api/wasmtime/component/macro.bindgen.html) | Host-side ownership modes, whole-interface/resource remapping, linking, traps, async, and debug output |

The current source is newer than the latest release. Claims about published behavior use `v0.63.0`; observations about current design name `a3533a2` explicitly.

## Missions and trust boundaries

| Concern | wit-bindgen guest bindings | witforge |
|---|---|---|
| Primary user | Code compiled into a WebAssembly component | Contract owner and native/process/schema consumers |
| Selected unit | One WIT world and its transitive imports/exports | Every version directory in a contract tree plus standard `deps/` packages |
| Rust callable surface | Imported free functions/resources; static exported `Guest` traits; generated `export!` ABI macro | Owned values and object-safe interface traits with `&self` |
| Binary boundary | Component Model Canonical ABI | Stable protobuf messages and method IDs; no service or session framing |
| Memory work | Lift/lower strings, lists, records, variants, resources, futures, and streams across linear memory | Construct owned Rust/prost values and perform checked structural conversion |
| Runtime | Guest ABI support only; the repository explicitly does not execute components | None |
| Host integration | Use Wasmtime or another Component Model runtime | Application-owned native/process/component adapters |
| Compatibility | WIT/component type and generator-version behavior | Append-only wire lock, frozen released versions, compatibility diff, generated version adapters |
| Schemas | Component type metadata embedded for the runtime | Protobuf and Draft 7 JSON Schema artifacts |
| Languages | Guest generators for Rust, C, C++, C#, Go, D, and MoonBit in the reviewed workspace | Rust values/traits plus language-neutral protobuf and JSON Schema |

The overlap is WIT loading, world/interface/type traversal, naming, deterministic generation, and Rust source emission. The outputs have different owners and invariants. Sharing those mechanics does not make either tool a substitute for the other.

## Pipeline and generator architecture

### wit-bindgen

The core `WorldGenerator::generate` algorithm receives a mutable `wit_parser::Resolve`, selects one `WorldId`, optionally creates nominal type IDs, and calls language-specific phases in this order:

1. preprocess;
2. imported interfaces, types, and functions;
3. finish imports;
4. exported world functions;
5. exported interfaces;
6. finish.

The shared `InterfaceGenerator` dispatches every current `TypeDefKind`, including resources, tuples, maps, fixed-length lists, futures, and streams. Language generators own their emitted files. `Files` uses a `BTreeMap`, so file iteration is deterministic, and `Source` centralizes indentation.

This is effective inside one coordinated 0.x workspace. It is not the right public third-party emitter contract for witforge: the trait exposes `wit-parser` IDs and structures directly, so every parser change reaches generator implementations. Witforge's opaque `Contract` and owned `Model` deliberately prevent that coupling.

`W11-001` should learn from the explicit traversal phases and deterministic file collector, but any future witforge emitter protocol must use the stable owned model or a versioned serialized protocol. It must not expose `wit-parser` as the plugin ABI.

### witforge

Witforge loads each released/draft version, enables all WIT features to prevent unstable members from silently disappearing, checks the restricted runtime-independent subset, builds the owned model, reconciles wire identity, and emits all requested projections. It reasons about a version tree rather than one selected world.

This architecture is intentionally stricter. A construct is accepted only when plain Rust, protobuf, JSON Schema where applicable, glue, locking, and evolution rules have an unambiguous shared meaning.

## WIT and package surface

| Surface | wit-bindgen 0.63/current | witforge 0.4.3 | Decision |
|---|---|---|---|
| Package resolution | `wit-parser::Resolve`; default `wit/`, inline WIT, one or multiple paths, encoded WIT, standard dependencies | One version directory at a time, standard `deps/`, opaque `Contract` and owned dependency model | Keep witforge's constrained layout; it is part of release reproducibility. |
| World selection | Selects one world; dependency worlds may be named fully | Emits all named worlds as inventory and all interfaces reached by the contract | Different product requirements. |
| World functions/types and inline interfaces | Generates bindings | Refused; use named interfaces | Keep refusal: lock keys, traits, and method identity are interface-scoped. |
| Records, variants, enums, flags, aliases, primitives, lists, options, results | Generated with Canonical ABI behavior | Generated across supported projections | Genuine overlap, different output contracts. |
| Tuples, maps, fixed-length lists, `char` | Generated | Refused with named/value alternatives | Do not add for parity; require an adopter and cross-projection rules. |
| Resources, `own`, `borrow` | Core Component Model surface | Refused | Permanent current boundary: these are runtime handles, not transport-neutral values. |
| Futures, streams, async functions, error context | Generated with async runtime support | Refused | Runtime concern; do not absorb into the contract compiler. |
| Accessors | Current generator supports them | Refused; use functions | Keep explicit function model unless all projections gain a stable meaning. |
| WIT stability features | CLI supports named features and `--all-features`; Rust macro supports an explicit feature list | Always enables every feature to preserve lockfile membership | Consumers combining the tools must align feature selection explicitly. Silent default omission is unsafe. |
| Versions | Generates bindings for the selected package/world version | Version directories, frozen releases, locked wire continuity, adjacent conversion | Witforge's main differentiation. |

Witforge should not chase the full WIT grammar. Unsupported constructs are often exactly where a runtime-neutral protobuf/plain-value projection stops being honest.

## Rust API and ergonomics

### What wit-bindgen provides

The Rust guest generator has a broad configuration surface because it is generating an ABI adapter:

- `owning`, deeply `borrowing`, and `borrowing-duplicate-if-necessary` type families;
- `with` remapping for interfaces and individual types, plus `generate_all`;
- selective function skipping and generated export stubs;
- global derives, derive exclusions, and targeted type/member attributes;
- alternate string bytes, map type, bitflags path, runtime path, and `std` feature gating;
- generated async/future/stream support and method chaining;
- unused-type selection and structural type merging;
- generated-source debugging through `WIT_BINDGEN_DEBUG`, macro `debug`, `cargo doc`, and the CLI.

The macro sorts `with` entries before building generator options and errors when configured selectors are unused. Those are good reproducibility and typo-detection practices.

### What witforge should retain

Witforge's generated Rust is deliberately boring:

- one owned type family;
- `Debug + Clone + PartialEq`, with opt-in Serde;
- object-safe `&self` traits with owned parameters and results;
- stable per-version modules and adjacent-version conversion;
- committed ordinary files that editors and rustdoc can inspect directly.

Borrowing, arbitrary derives, remapping, stubs, async, and `no_std` are not missing parity. Borrowing and conditional derives already have measured consumer-triggered revisit rules in [`overview.md`](overview.md#101-borrowed-rust-parameter-measurement-w6-002) and [`overview.md`](overview.md#102-conditional-eqhash-and-default-evaluation-w6-003). Arbitrary type remapping would also weaken the promise that every emitter projects the same WIT-owned values.

Practices worth retaining or adopting when relevant:

- fail on unused configuration selectors rather than silently accepting typos;
- sort all map-derived generation decisions before emission;
- keep generated source easy to inspect;
- explain a missing remapping/configuration choice with concrete alternatives;
- test every supported naming collision and language keyword explicitly.

Witforge already satisfies the first three structurally through `deny_unknown_fields`, ordered collections, committed output, ownership headers, and collision tests. No parity task is needed.

## Naming, diagnostics, and output ownership

wit-bindgen namespaces guest Rust by WIT package/interface and separates exports under `exports`. It has dedicated collision/keyword WIT fixtures. Macro parse/resolve failures are rendered with WIT source maps at the invocation span, and a missing `with` mapping explains the available remedies.

The CLI:

- selects a world and accepts directories, files, or encoded WIT;
- writes generator-owned output and supports `--check`;
- distinguishes line-ending-only drift from other stale output;
- writes a preamble containing the wit-bindgen version and non-default options.

It does not use witforge's output-root ownership protocol: witforge refuses to overwrite unowned files, removes only stale files carrying the matching emitter header, rejects symlinks, and emits stable content independent of the installed patch version. Including the generator version in witforge output would make every tool release rewrite otherwise identical frozen output, so witforge must not copy that preamble policy.

A line-ending-specific `--check` diagnostic is a reasonable low-risk ergonomic idea, but no consumer has reported this friction and it does not justify a task. The trigger is a real stale-output incident where the generic “changed generated file” message delays diagnosis.

## Compatibility and release policy

The reviewed wit-bindgen repository is version `0.63.0`, and the latest release is also `v0.63.0`. The README, Cargo manifests, macro documentation, and release workflow reviewed here do not publish a 1.x-style compatibility promise for the CLI or generated Rust API. Therefore this audit does **not** label either surface “unstable” as a sourced project policy; it records the narrower facts:

- the crates remain on semver 0.x;
- generated code depends on generator options, WIT features, and the exact generator implementation;
- current main contains unreleased generated-language changes after `v0.63.0`;
- CI verifies current generation and execution, not compatibility with historical generated APIs.

Consumers should pin wit-bindgen, regenerate deliberately, and compile/run their component tests on upgrades.

Witforge's intended 1.0 contract is different: the public library API, generated layout, released version modules, wire numbers, and conformance suite have documented compatibility and deprecation rules. A future integration must not expose wit-bindgen or Wasmtime generated types from witforge's stable API, because doing so would transitively couple that promise to another generator's release cadence.

## Testing models

| Evidence | wit-bindgen | witforge | Lesson |
|---|---|---|---|
| Construct breadth | Large shared WIT corpus covering full Component Model constructs, keywords, names, packages, and issue regressions | Restricted-subset fixtures and a conformance matrix | Keep witforge's explicit matrix; borrow new edge-case fixtures only within its subset. |
| Code generation | Generated code is compiled by each language toolchain; Rust options also have direct compile tests | Exact text goldens plus generated Rust/protobuf/glue compilation | Keep goldens because output stability is a witforge promise; compile tests catch different failures. |
| Runtime | Components are compiled and executed against generated hosts across supported languages | Protobuf/glue binary and JSON round trips; no component runtime | Do not add component runtime tests until witforge owns an integration surface. |
| Cross-language | CI covers Rust, C, C++, C#, Go, and D; MoonBit exists but is currently omitted from the language matrix | Protobuf and JSON Schema are language-neutral; implementation tests are Rust | A future multi-host kit should run semantic fixtures across hosts, not add guest-language generators to witforge. |
| Tool modes | Feature combinations, `no_std` targets, Miri, platform builds, MSRV 1.88 | Linux/macOS/Windows, MSRV 1.88, clippy/rustdoc/security/release gates | No material baseline gap. |
| Historical compatibility | No historical generated-output compatibility gate found | Historical generator protobuf comparisons and release-freeze checks | Preserve witforge's stronger historical gates. |

The most valuable transferable pattern is semantic runtime conformance: if two real applications eventually justify a separate component/process/native kit, the same domain cases should execute against all hosts. That belongs to `W11-030`, not witforge's current emitter suite.

## Bridge evaluation

### Guest Rust: direct type remapping works

A disposable exercise used witforge 0.4.3 and wit-bindgen current commit `a3533a2`:

1. Define `audit:bridge/api@1.0.0` with an owned record containing `string` and `list<u32>`, a payload variant, and an exported function.
2. Generate witforge's `plain::api::{Item, Outcome}`.
3. Invoke `wit_bindgen::generate!` with versioned type selectors mapping `item` and `outcome` to those plain types.
4. Implement the generated `Guest` trait using the witforge types and invoke generated `export!(Component)`.
5. Run `cargo check` successfully.

The generated `Guest::roundtrip` signature used the exact witforge `Item` and `Outcome` paths. The generated ABI lowering destructured those values and moved their `String`/`Vec` storage into the Canonical ABI representation. There was no intermediate application adapter and no extra nested clone introduced by a bridge.

This is the preferred first integration technique for compatible guest-side owned values. Its constraints are explicit:

- only the shared witforge subset is relevant;
- remapped structs and variants must expose the expected public fields and cases;
- selectors include WIT package/interface versions and are coupled to wit-bindgen's configuration syntax;
- Canonical ABI lifting/lowering and its memory ownership still occur;
- resources, futures, streams, borrowed type families, and other runtime constructs remain wit-bindgen-owned.

A documentation recipe can be published after a real adopter uses it. A permanent witforge test would add a fast-moving 0.x dev dependency before witforge owns an integration contract, so none is added now.

### Wasmtime hosts: application adapters remain necessary

Wasmtime host `component::bindgen!` supports remapping complete previously generated interface/package modules and resource representation types. Its documented `with` option does not remap arbitrary record and variant types to a runtime-independent plain module. A witforge module also lacks Wasmtime's generated `Host` traits, linker hooks, typed component wrappers, store access, trap configuration, and async machinery.

For a host application that uses both surfaces, an adapter therefore remains the boring design:

- scalars and owned strings/bytes can generally move without cloning;
- a record or variant conversion reconstructs the outer Rust value but can move owned leaf storage;
- nested lists whose element type changes require traversal and normally a new vector allocation;
- resource and async values have no witforge representation.

Generating this adapter would couple witforge output to Wasmtime's generated API and could add allocation for nested structures. The trigger for reconsideration is a named adopter where direct guest remapping is insufficient, a stable pinned runtime surface is available, and measured conversion cost or maintenance duplication is material. Any task must include representative nested payload benchmarks, allocation counts, compile tests against pinned versions, and a decision about package ownership outside witforge core.

## Prioritized disposition

| Priority | Decision | Trigger or owning task |
|---|---|---|
| P0 | Keep the explicit boundary: wit-bindgen owns guest Canonical ABI; Wasmtime owns host execution; witforge owns runtime-independent contracts and evolution. | Adopted in this document; feed candidate reviews and `W11-029`. |
| P0 | Prefer guest `with` type remapping over a new bridge emitter when a Component Model guest shares witforge-owned value types. | Document publicly only after an independent adopter proves the recipe. |
| P0 | Require WIT feature-set alignment when combining tools; witforge enables all features while wit-bindgen defaults require explicit feature selection. | Candidate exercises and the future adopter guide. |
| P1 | Use wit-bindgen's phased traversal and deterministic file collector as prior art, but keep `wit-parser` out of any stable external emitter ABI. | Existing `W11-001`; no new task. |
| P1 | Reuse semantic fixtures across native, component, and process implementations only after repeated real applications justify a separate kit. | Existing `W11-030`. |
| P2 | Consider a line-ending-specific stale-output diagnostic after observed user friction. | No task until an incident exists. |
| Deferred | Targeted attributes, extra derives, selective generation, type merging, and alternate map/string representations. | A concrete consumer request plus a new-version compatibility plan. |
| Declined | Canonical ABI shims, guest exports, Component Model resources/async, host linkers, runtime execution, and guest-language generators in witforge. | Outside witforge's mission. |
| Declined | A generic generated witforge↔wit-bindgen/Wasmtime conversion layer today. | Reconsider only under the measured adopter trigger above. |

No additional implementation task is warranted by this audit. Existing tasks already own the only evidence-backed follow-ups.

## Observed verification

The following was exercised during the audit:

- `wit-bindgen` current source was cloned at `a3533a2`; release `v0.63.0` was resolved to `29e0393`.
- `wit-bindgen` Rust CLI generated bindings for the disposable contract and its `--check` mode accepted the unchanged output.
- witforge 0.4.3 locked the disposable WIT and generated the plain Rust record, variant, and trait.
- a scratch Rust crate remapped both WIT types to witforge output, implemented the generated guest trait, expanded the ABI into a debug file, invoked `export!(Component)`, and passed `cargo check`.
- the expanded signature and lowering code were inspected to distinguish direct type reuse from unavoidable Canonical ABI lift/lower.
- `cargo test -p wit-bindgen-core --locked` passed 5 tests, and `cargo test -p wit-bindgen-rust --locked` passed 2 direct generator tests at the reviewed current commit.

The scratch projects are not adoption evidence and are not retained in this repository.