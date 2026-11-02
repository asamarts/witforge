# witforge design overview

Status: living design document (2026-10-14; decisions through 2026-11-01 are logged in section
9). The internal model, lockfile format, and Rust/protobuf/glue emitter conventions are specified
normatively in [`model.md`](model.md), [`lockfile.md`](lockfile.md), and
[`emitters.md`](emitters.md). The decision to build witforge, and its role in alint's plugin
architecture, are recorded in
alint's
[ADR-0019](https://github.com/asamarts/alint/blob/main/docs/adr/0019-general-linter-program-and-plugin-architecture.md)
and [`plugin-architecture.md`](https://github.com/asamarts/alint/blob/main/docs/design/plugin-architecture.md)
(sections 6 to 8). This document is canonical for witforge's architecture; the linked
specifications are normative for their areas.

## 1. Purpose and scope

witforge compiles one hand-written WIT package into the bindings other implementations of the
contract need: plain Rust types and traits, a protobuf schema with stable field numbers, and
Draft 7 JSON Schema. It does not generate WASM guest or host bindings (`wit-bindgen` and
`wasmtime::component::bindgen!` already do that) and it does not implement any transport.

Out of scope for 0.1: services and RPC frameworks, every target language, WIT resources,
streams, and futures.

## 2. Pipeline

```text
WIT directory
  -> wit-parser Resolve (all features enabled)      load
  -> subset checker                                 subset::check
  -> internal model (types, docs, stability)        model
  -> wire lockfile reconcile / drift check          lock
  -> emitters: Rust, protobuf, JSON Schema, glue   emit
```

`wit-parser` is pinned to an exact version: it makes breaking 0.x releases roughly monthly and its
enums are exhaustive. The internal model isolates the emitters from its types, so a bump touches
one module. The model is specified in [`model.md`](model.md).

## 3. The subset (version 1)

Refused, each with the supported alternative:

| Construct | Why | Use instead |
|---|---|---|
| `resource`, `own`, `borrow` | no value representation on the wire; the contract is pure functions over values | records and ids |
| `future`, `stream` | the contract is synchronous | lists |
| `error-context` | part of async; the contract is synchronous | a record or variant describing the error |
| `async` functions | the contract is synchronous | a plain function |
| `get` and `set` accessors | no protobuf or plain-Rust counterpart | a function |
| `map<K, V>` | new in WIT; protobuf maps restrict keys and values | `list<record>` |
| fixed-length lists | no protobuf equivalent | `list` |
| `char` | a one-scalar string on the wire, error-prone | `string` |
| tuples | positional, unnamed fields | a named record |
| empty records | rejected by WIT tooling; no useful message | a variant case without payload |
| types defined in a world | lockfile keys are per interface | define the type in an interface |
| functions defined in a world, inline interfaces | traits, method ids, and lockfile keys are per named interface | a named interface the world imports or exports |
| types or interfaces from another package | the lockfile and the emitters cover one package, so a foreign type would have no numbers and no bindings | define the type in the contract's package |
| an unversioned package | `@since` gates and evolution checks need a version | `package ns:name@x.y.0;` |
| a version other than `<major>.<minor>.0` (patch, pre-release, build metadata) | one version directory maps to one Rust module and one proto major; patch and pre-release variants are out of scope for 0.1 | `package ns:name@x.y.0;` |
| anything unrecognised | a new `wit-parser` construct must not slip into generated code | extend the checker deliberately |

A `use` in a world is allowed: it names a type defined in an interface and defines nothing.
`wit-parser` already rejects empty records (and empty variants, enums, and flags) at parse time;
the checker keeps its own empty-record rule in case a `wit-parser` bump stops doing so.

## 4. Type mapping

| WIT | Rust | proto3 | proto3-JSON | Status and convention |
|---|---|---|---|---|
| `bool` | `bool` | `bool` | true / false | clean |
| `u8`, `u16` | `u8`, `u16` | `uint32` | number | range-checked on decode |
| `u32` | `u32` | `uint32` | number | clean |
| `u64` | `u64` | `uint64` | decimal string | readers also accept a number |
| `s8`, `s16` | `i8`, `i16` | `sint32` | number | range-checked on decode |
| `s32`, `s64` | `i32`, `i64` | `sint32`, `sint64` | number, string | clean |
| `f32`, `f64` | `f32`, `f64` | `float`, `double` | number or "NaN" / "Infinity" | clean |
| `char` | `char` | `string` of one scalar value | string | refused by the subset checker; use `string` |
| `string` | `String` | `string` | string | UTF-8 on both sides |
| `list<u8>` | `Vec<u8>` | `bytes` | base64 | clean |
| `list<T>` | `Vec<T>` | `repeated singular(T)` | array | wraps nested list/option elements |
| `list<list<T>>`, `list<option<T>>` | nested | repeated wrapper message, or `repeated bytes` when the inner type is `list<u8>` | array of objects, or array of base64 strings | synthesized wrapper |
| `option<T>` (scalar, string, enum) | `Option<T>` | `optional T` | absent or null | clean |
| `option<T>` (record) | `Option<T>` | message field | absent or null | presence-checked on decode; required scalars use `optional` |
| `option<list<T>>`, `option<option<T>>` | nested | wrapper message, or `optional bytes` for `option<list<u8>>` | object, or base64 string | synthesized wrapper |
| `result<T, E>` | `Result<T, E>` | message with `oneof value { singular(T) ok = 1; singular(E) err = 2; }`; `_` payloads are `Empty`; list/option payloads are wrapped to a singular message | `{"ok": ...}` / `{"err": ...}` | one-sided and bare results use `_` |
| `tuple<A, B>` | `(A, B)` | message with `_0`, `_1` | object | refused by the subset checker; use named records |
| `record` | `struct` | message, numbers from the lockfile | object, lowerCamelCase keys | clean once locked |
| `variant` with payload | `enum` with data | message with `oneof witforge_value`, numbers from the lockfile | `{"<case>": ...}` | a list or option payload needs a wrapper; an unknown case is a decode error |
| `variant` case without payload | unit variant | `Empty` in the `oneof` | `{"<case>": {}}` | convention |
| `enum` | `enum` | `<ENUM_NAME>_UNSPECIFIED = 0`, then locked cases | name string | zero or an unknown value is a decode error (closed enums) |
| `flags` | struct of bools | message of bools | object | preferred over a bitmask (readable JSON, no 64-flag cap) |
| `resource`, `own`, `borrow` | handle | opaque id message | n/a | refused by the subset checker |
| `type a = b` | `pub type` | inlined | n/a | clean |
| `map<K, V>` | `BTreeMap` | `map<K, V>` for string or integer keys | object | refused by the subset checker; use `list<record>` |
| `future`, `stream`, fixed-length lists | n/a | n/a | n/a | refused by the subset checker |
| function | trait method | request message (parameters numbered from the lockfile) and response message, plus a method id | n/a | convention |
| interface | trait or module | method-id enum, numbers from the lockfile | n/a | convention |
| package `ns:name@x.y.0` | Rust module `v<major>_<minor>` | proto package `ns.name.vX` | n/a | major version only on the protobuf side |

Presence: every **required** scalar, string, bool, float, enum, or `list<u8>` occurrence in a
record field, request parameter, or response result is emitted as proto3 `optional`, and the
glue rejects absence with `MissingField`. A WIT `option<T>` uses the same optional/wrapper shape
but permits absence. Required message fields use message presence; required `repeated` fields
may be empty; oneof selection carries presence for result and variant payloads. The schema cannot
tell a required `optional T` from `option<T>`; the model does, so the glue applies the right
rule.

### 4.1 Generated Rust

Each version directory is generated into its own module (`v0_1`, `v0_2`, and so on) of plain,
exhaustive structs and enums, without `#[non_exhaustive]`: plugin authors build values with
struct literals and match exhaustively. Within a version module, types are grouped into one
submodule per defining interface, and interface traits are re-exported at the version root. A
released module never changes (its WIT is frozen), so a crate of generated modules stays
semver-compatible by adding modules. `rust` also writes `convert.rs` with the old-to-new and
new-to-old adapters of section 5.3, and `worlds.rs` with additive world metadata. The concrete layout, naming, derives, and trait conventions
are in [`emitters.md`](emitters.md).

### 4.2 Protobuf glue

An optional output converts between the plain Rust types and the types `prost` generates from
the emitted `.proto` (the consumer runs `prost-build`, through `protox` so no `protoc` binary is
needed, and `pbjson-build` for proto3-JSON). The conversions carry every decode rule of the
table above: range checks for `u8`, `u16`, `s8`, and `s16`; closed enums (zero or an unknown
value is an error); presence checks for every required field, including proto3 `optional`
scalars, strings, bytes, and enums (absence is `MissingField`); and an error for an unknown
variant case. So the mapping lives in one place, and witforge's round-trip tests use the same
code its consumers do. `prost` is pinned and bumped deliberately, like `wit-parser`. The
protobuf schema and generated-file conventions are in [`emitters.md`](emitters.md); the glue
decode rules and its free-function API are specified there.


## 5. Wire identity and evolution

### 5.1 Why the strictest system governs

wasmtime checks component types for exact equality (verified in the 49.x source: record field
count, order, and names; variants, enums, and flags compared exactly). So in the component model
adding a record field, a variant case, an enum value, or a flag is a breaking change, and only
patch versions of a `0.x` package may differ, and then only in docs. Protobuf tolerates new
fields by stable number, renames are wire-compatible but break proto3-JSON, and renumbering or
retyping is breaking. Rust tolerates new fields with `#[non_exhaustive]`. The contract follows
the strictest rule. witforge 0.1 does not support patch contract versions in the tree: a change
ships as a new `v<major>.<minor>` directory (see [`lockfile.md`](lockfile.md)).

### 5.2 Stable field numbers: the lockfile

WIT has no field numbers, so the compiler keeps a committed, append-only `wire.lock` that maps
each numbered member to a number unique within its parent. The normative format and
`check`/`lock` semantics are in [`lockfile.md`](lockfile.md).

| Member | Key | Parent (numbers unique within) | Entry kind |
|---|---|---|---|
| record field, variant case, enum case, flag | `ns:pkg/interface/type.member` | the type | `field`, `variant-case`, `enum-case`, `flag` |
| function (its method id) | `ns:pkg/interface.function` | the interface | `method` |
| function parameter | `ns:pkg/interface#function.parameter` | the function | `param` |

Function method ids and parameters are specified (decided 2026-10-24, format fixed 2026-10-25)
and implemented in v2, including draft/release modes, `--first`, `--against`, and `--released`.
The rules in this section describe the implemented format.

- each entry records its member's kind and, where it has one, its type in canonical form
  (aliases and named list/option/result definitions expanded, named record/variant/enum/flags by
  `interface/type`, no spaces), for example `ns:pkg/iface/span.start = 1 field u64`; a method-id
  entry records the function's result type; enum cases, flags, and payload-less variant cases
  record no type. `check` fails when a kind or type differs, which catches changes the protobuf
  projection cannot see (`u8` to `u16` are both `uint32`, and a payload-less variant can become
  an enum without changing names);
- a new member gets the next free number (maximum existing number plus one, skipping protobuf's
  reserved 19000 to 19999 range); numbers are never reused;
- a removed member is emitted as `reserved` (number and name) with its kind and type retained,
  and its number is never reused;
- a member that comes back under a removed released member's name is refused (`check` fails and
  `lock` writes nothing), because the emitted schema reserves the name as well as the number;
  a removed draft leaves no reservation and may return;
- names are identity, as they are in proto3-JSON, the component model, and Rust: a rename is a
  removal plus an addition, and there are no rename markers. A badly named published member is
  replaced by adding a new member, deprecating the old one in its docs, and removing it at the
  next major version;
- each version directory has its own lockfile, started as a copy of the previous version's.
  `check` and `lock` with `--against <previous version's wire.lock>` prove continuity (no entry
  removed, renumbered, un-reserved, re-kinded, or retyped) and apply the draft rule: a member
  that is not in the previous version's lockfile is a draft, and renaming or removing it drops
  its entry instead of reserving it, so names that never shipped leave no `reserved` gaps.
  `--first` bootstraps a version with no previous lockfile (the first version or a new major)
  with every entry treated as a draft. The generators verify the same continuity between
  consecutive minors of each major, and default-mode `lock` and `check` refuse a missing
  lockfile, so a lockfile started fresh instead of copied cannot renumber a major's schema;
- `wit/released` lists released version directories; their version directories are frozen
  (shared `mod.rs`, `convert.rs`, `worlds.rs`, proto, and glue output changes as described in section 5.3).
  `lock` refuses them, and CI fails if anything in a released version directory changed after
  the commit that released it. A new minor is developed with the previous version as its
  `--against` baseline;
- CI fails if a member is unlocked, if a released directory changes, or if generated output is
  stale; `buf breaking` on the emitted schema is an optional extra check;
- the lockfile is designed to be replaced by in-source numbers if the WIT annotations proposal
  (component-model issue 695) lands with a suitable annotation.

### 5.3 Evolution policy

1. A published contract version is frozen. `wit/released` lists the released version
   directories; their WIT, lockfile, and per-version generated Rust module are immutable. The
   shared `mod.rs`, `convert.rs`, `worlds.rs`, per-major proto schema, and glue output may change when a new
   minor is added; their released members are protected by the lockfile and `--check`. In the proto schema the
   wire numbers and reserved names are append-only; a type or function removed as a whole drops
   its messages and keeps its method number reserved.
2. Changes ship as a new minor version (`0.x` while unstable), with the previous WIT kept in the
   tree (`wit/v0.1/`, `wit/v0.2/`, and so on), each with its own `wire.lock` (section 5.2). A new
   major version starts a fresh lockfile. Package versions are `<major>.<minor>.0`; patch,
   pre-release, and build-metadata contract versions are out of scope for 0.1 (see
   [`lockfile.md`](lockfile.md)).
3. Authoring policy for a new minor: new members should be defaulted (`option<T>` or `list<T>`),
   so a later version adapter can convert old values to new ones; enum values and variant cases
   should only be appended; reorders, renames, and type changes should be avoided. `rust` enforces
   the defaultable-addition rule for generated conversions: a required added record field without
   a default is refused, while proto and glue can still emit the schema. The tool enforces wire
   identity: numbers never change, removed members stay reserved, and a released entry's kind and
   type never change (the lockfile, section 5.2). The lockfile also records per-parent
   declaration-order fingerprints in an additive section; `--against` refuses a reorder of
   type members, functions, or parameters common to two versions while ignoring additions and
   removals
   ([`lockfile.md`](lockfile.md) section 16).
4. `rust` generates `convert.rs` for every adjacent version pair, in both directions, and the host
   keeps one adapter per supported contract version or converts each into the latest plain types;
   the supported range is published. The conversions default additions, report removed required
   members, and refuse retypes and kind changes ([`emitters.md`](emitters.md) section 9).
5. On the process wire, one protobuf package per major version; field numbers are stable across
   minors through the lockfile, so older process plugins keep working, and the handshake still
   states the exact contract version so the host enforces its semantics.
6. A major version is a new package (for example `ns:name@2`, protobuf `ns.name.v2`), negotiated at
   handshake, with an adapter for the previous major for a documented period.
7. For alint's contract, 1.0 freezes at alint v1.0 together with its DSL (alint decision DEC-9).


## 6. Prototype findings and `wit-parser` gotchas

A throwaway prototype (about 290 lines on `wit-parser` 0.262, 2026-10-14) rendered a sample
contract (records with spans, a variant with payload, payload-less, and tuple cases, enums,
flags, a resource, `list<u8>`, nested `option` and `list`, `result`, `@since` and `@unstable`
items) to proto3 with lockfile-assigned numbers, and kept numbers stable across a rename and a
package version bump. The API exposes everything a generator needs (packages with versions,
interfaces, worlds, type definitions, docs, stability). Gotchas found:

1. Record fields, variant cases, enum cases, and flags carry no stability attribute; only types,
   functions, interfaces, and worlds take `@since`. Per-member evolution needs versioned
   interfaces (section 5.3).
2. `@since(version = X)` may not exceed the package version; bump the package first.
3. `@unstable` items are filtered out unless all features are enabled.
4. `use` creates alias definitions, and `type x = list<T>` is a named list definition, not an
   alias; the generator must dedupe and inline both.
5. Lockfile keys must exclude the package version, or every bump renumbers everything.
6. Anonymous types (`result`, tuples, nested `list` and `option`) need synthesized message names;
   these affect only generated code, never the wire or JSON, so they are not locked.
7. Function names arrive mangled (`[method]x.y`); use the unmangled item name.
8. `wit-parser` makes breaking 0.x releases roughly monthly and its enums are exhaustive; pin it
   and keep the subset narrow.


## 7. Testing strategy

- Golden tests: one fixture contract per mapping row, with committed expected Rust, protobuf,
  JSON Schema, and glue output; at least one generated Rust/glue fixture is compiled, one proto
  schema is parsed, and Draft 7 schemas validate representative valid and invalid instances.
- Round-trip tests: generated Rust values encoded to protobuf and proto3-JSON and decoded back;
  glue conversions and per-function request/response helpers are exercised through the same path.
- Lockfile property tests with `proptest`: reconcile is idempotent, never renumbers, and the
  append-only/draft rules hold across random sequences of member additions, removals, renames,
  and type changes. The cases are listed in [`lockfile.md`](lockfile.md#14-testing-requirements).
- CLI integration tests that run the built binary with `std::process::Command` in directories
  under `std::env::temp_dir()`, for `check`, `lock`, `--against`, `--first`, `--released`,
  `rust`, `proto`, `glue`, and `--check`.
- Subset tests: every refused construct has a fixture (`tests/fixtures/refused/<construct>/`)
  with the exact violations it must produce.
- CI: MSRV 1.88; `cargo fmt`, pedantic Clippy, and rustdoc with warnings denied; Linux, macOS,
  and Windows on GitHub-hosted runners. Before 0.1.0, while the repository was private, CI ran
  on one self-hosted Linux runner (`ci/README.md`).

## 8. Relationship to alint

alint is witforge's first consumer. Its plugin contract `0.1` (still a draft) is generated with
witforge into a test crate, which checks the plain types, the `.proto` schema, and the glue
against alint's hand-written host types (alint PRs #409, #412, #414, and #415); the repository went
public for 0.1.0.

alint consumes only published releases. Its CI installs the release it pins (0.1.2 since
2026-11-01) from crates.io, checks the contract, and regenerates the committed output with
`--check`, so witforge's `main` can change without breaking alint, and alint sees a change only
when it moves the pin. A change to generated output or to what `check` accepts therefore reaches
alint as a pull request there that bumps the pin and commits the regenerated files, and it needs
a witforge release first; each changelog section says whether that release changes generated
output. When alint's plugin slice ships, witforge generates the published
`alint-plugin-api` crate and the `.proto` schema its out-of-process plugin protocol uses, and the
conversions between the two (section 4.2). alint keeps the host glue: wasmtime bindings, the
process protocol's framing and session, and the version adapters.

## 9. Decision log

| Date | Decision | Where |
|---|---|---|
| 2026-10-24 | The subset refuses functions defined in a world, inline interfaces, `async` functions, `get`/`set` accessors, and anything from another package | section 3 |
| 2026-10-24 | Function method ids and parameters are locked like type members | section 5.2 |
| 2026-10-24 | Lockfile entries record their member's type; `check` fails on a type change, and `buf breaking` becomes optional | section 5.2 |
| 2026-10-24 | One lockfile per version directory, with `--against` the previous version's | sections 5.2 and 5.3 |
| 2026-10-24 | Names are identity: no rename markers; drafts (members not yet released) may be renamed or removed freely | section 5.2 |
| 2026-10-24 | Generated Rust is frozen, exhaustive types in one module per version | section 4.1 |
| 2026-10-24 | witforge emits the conversions between the plain types and `prost` types, with the decode rules | section 4.2 |
| 2026-10-24 | Rust edition 2024; CI on a self-hosted Linux runner while the repository is private | `Cargo.toml`, `ci/README.md` |
| 2026-10-25 | Lockfile v2: versioned header, `key = number kind [type] [reserved]` entries, explicit member kinds; reserved entries keep kind and type | [`lockfile.md`](lockfile.md) |
| 2026-10-25 | The internal model is owned and independent of `wit-parser`, preserves source order/docs/stability/aliases, and exposes canonical types and lockfile members | [`model.md`](model.md) |
| 2026-10-25 | Canonical types fully qualify named types as `interface/type`, expand aliases and named list/option/result structurally, and spell both result slots with `_` | [`lockfile.md`](lockfile.md) |
| 2026-10-25 | `--against` is development-only; `--first` bootstraps a version with no previous lockfile (the first version or a new major); `wit/released` lists frozen version directories and per-version Rust modules, while shared output evolves append-only | [`lockfile.md`](lockfile.md) |
| 2026-10-25 | Declaration order is not tracked in the MVP; a pre-golive task evaluates the order-fingerprint option (superseded 2026-11-01) | section 5.2, `ROADMAP.md` |
| 2026-10-25 | `@unstable` and `@since` are metadata; every item in a version directory is part of that version | [`lockfile.md`](lockfile.md) |
| 2026-10-25 | Functions get per-function request/response messages; the response always has `result = 1`; result oneofs use `Empty` for missing payloads | [`emitters.md`](emitters.md) |
| 2026-10-25 | Generated Rust groups types into per-interface submodules, re-exports interface traits at the version root, and uses `&self` with owned parameters/returns and no extra bounds | [`emitters.md`](emitters.md) |
| 2026-10-25 | Generated Rust derives `Debug`, `Clone`, and `PartialEq`; `Eq`, `Hash`, and `Default` are deferred | [`emitters.md`](emitters.md) |
| 2026-10-25 | Rust name conversion uses raw identifiers where possible, a trailing `_` for `self`/`Self`/`crate`/`super`, fully qualified `::std` paths, and hard errors on collisions | [`emitters.md`](emitters.md) |
| 2026-10-25 | Protobuf emits one file per major, a flat interface-prefixed package, a shared `Empty`, and structural projections for named list/option/result definitions (named messages where required) | [`emitters.md`](emitters.md) |
| 2026-10-25 | Anonymous protobuf wrappers use deduplicated structural `Witforge...` names; enum and method values are prefixed with `UNSPECIFIED = 0`; reserved members emit number and proto name | [`emitters.md`](emitters.md) |
| 2026-10-25 | Named `option`/`list` types keep their name for the direct wrapper form; a differing oneof-singular form uses an anonymous `Witforge...` wrapper | [`emitters.md`](emitters.md) |
| 2026-10-25 | Generation takes the WIT root; `rust` and `proto` are separate subcommands with fixed output layout and header-owned `--check` semantics | [`emitters.md`](emitters.md) |
| 2026-10-25 | Version directories are `v<major>.<minor>`, package versions are `<major>.<minor>.0`, Rust modules are `v<major>_<minor>`, proto packages are `v<major>` | [`lockfile.md`](lockfile.md) |
| 2026-10-25 | CLI uses `clap`; lockfile properties use `proptest`; CLI tests use `assert_cmd` and `tempfile`; golden expectations are plain committed files | section 7, `Cargo.toml` |
| 2026-10-25 | Worlds generate no Rust or protobuf code in the MVP; world metadata is a later option (superseded 2026-11-01) | [`emitters.md`](emitters.md) |
| 2026-10-26 | Glue uses free functions per named type, grouped by interface and type, with infallible `to_proto` and fallible `from_proto`; every named type in the highest minor is covered | [`emitters.md`](emitters.md) |
| 2026-10-26 | Glue covers only the highest minor of each major; older plain modules are for native adapters and the generated version conversions | [`emitters.md`](emitters.md) |
| 2026-10-26 | Glue has a separate output root with required `--plain-root` and `--proto-root`, one `v<major>.rs` per major, and the same `--check`/ownership rules | [`emitters.md`](emitters.md) |
| 2026-10-26 | Glue generates per-function `to_request`/`from_request`/`to_response`/`from_response` helpers and a `METHOD` constant, with ergonomic zero/one/many arity and functions under a `functions` module | [`emitters.md`](emitters.md) |
| 2026-10-26 | Glue returns one structured, non-exhaustive `GlueError`; JSON and binary encoding stay on the pbjson/prost side | [`emitters.md`](emitters.md) |
| 2026-10-26 | Required scalar/string/bool/float/enum/bytes fields, parameters, and response results use proto3 `optional` and reject absence; `option<T>` permits absence | [`emitters.md`](emitters.md) |
| 2026-10-29 | 0.1 generation baseline and ergonomics measured; deferred options assigned to named tasks with triggers, without accepting or implementing any option here | section 10 |
| 2026-10-31 | CLI tests run the built binary with `std::process::Command` under `std::env::temp_dir()`, as implemented, instead of `assert_cmd` and `tempfile` (no extra dev-dependencies); supersedes that part of the 2026-10-25 entry | section 7 |
| 2026-10-31 | CI runs on GitHub-hosted runners from 0.1.0; the aplan plan is checked locally until aplan is published (W5-006) | [`ci/README.md`](../../ci/README.md) |
| 2026-10-31 | Default-mode `lock` and `check` refuse a missing lockfile, and the generators require each minor's lockfile to continue the previous minor's | [`lockfile.md`](lockfile.md) |
| 2026-10-31 | Every command exits `1` for a contract, lockfile, or output violation and `2` for a usage, I/O, parse, released-directory, or internal error; the library marks violations with `ContractError` | [`emitters.md`](emitters.md) |
| 2026-10-31 | Released-manifest entries are plain directory names (`^[A-Za-z0-9][A-Za-z0-9._-]*$`); the released-directory and output-root refusals compare resolved paths; the release freeze reads the manifest's history instead of one merge base | [`lockfile.md`](lockfile.md) |
| 2026-10-31 | Releases publish from a `v*` tag through crates.io trusted publishing; a new crate's first version is published by hand with a scoped token | [`ci/README.md`](../../ci/README.md) |
| 2026-11-01 | Version directories are never symlinks: every command, the released manifest, and the freeze check refuse one, instead of following it; for `lock`, the manifests beside the version directory and the lockfile always apply and `--released` adds to them | [`lockfile.md`](lockfile.md) |
| 2026-11-01 | `check` requires the canonical lockfile rendering; a contract is one package, so a nested package or a dependency is refused | [`lockfile.md`](lockfile.md) |
| 2026-11-01 | `check` reads CRLF as LF and requires the canonical lockfile only for an unreleased version, which `lock` can rewrite; a released lockfile is kept as released by the freeze check (supersedes that part of the previous entry). A directory named `v<major>.<minor>` holds that version for every command, and `glue` refuses what `proto` refuses | [`lockfile.md`](lockfile.md) |
| 2026-11-01 | `rust` emits `convert.rs` for every adjacent version pair, in both directions; conversions default `option`/`list` additions, report removed required members and unknown cases, and refuse retypes and kind changes | section 5.3, [`emitters.md`](emitters.md) section 9 |
| 2026-11-01 | The lockfile gains an additive per-parent declaration-order fingerprint section; `--against` refuses reorders of common type members and parameters, ignoring additions and removals, and old v2 lockfiles migrate on the next `lock` | [`lockfile.md`](lockfile.md) section 16 |
| 2026-11-01 | JSON Schema is a Draft 7 value projection: one file per version, all named types under `definitions`, all record fields required, closed enums, one-of result/variant cases, no runtime dependency | [`emitters.md`](emitters.md) section 10 |
| 2026-11-01 | `witforge.toml` selects projects explicitly; config paths resolve from the file, CLI flags override every path, unknown keys are errors, and CLI-only invocation remains valid | [`emitters.md`](emitters.md) section 11 |
| 2026-11-01 | Owned Rust parameters remain for 0.x and 1.0; measurements show clone cost but borrowing would require a second lifetime-bearing generated surface and does not remove native-to-WIT mapping. Revisit with a measured consumer path and a new contract version | section 10.1, [`emitters.md`](emitters.md) section 3.5 |
| 2026-11-01 | Conditional `Eq`/`Hash`/`Default` are declined for 0.x/1.0; derives cannot be added retroactively to frozen version APIs, floats block Eq/Hash transitively, and WIT has no natural Default for enums/variants/results. Revisit with a consumer request and a new contract version | section 10.2, [`emitters.md`](emitters.md) section 3.3 |
| 2026-11-01 | Worlds get additive shared `worlds.rs` metadata with per-version `WORLDS` constants, explicit imports/exports and no traits; per-version modules remain frozen | [`emitters.md`](emitters.md) section 3.6 |
| 2026-11-01 | Docs-only patch versions and a memberless lockfile inventory are declined for 0.x/1.0; draft docs changes are accepted, memberless additions have no wire entry, and released docs are freeze-guarded | section 10.3, [`lockfile.md`](lockfile.md) section 15 |
| 2026-11-01 | 1.0 keeps `anyhow::Error` at the boundary with the `ContractError` classification; the public API, conformance matrix, release criteria, and deprecation runways are published | [`public-api.md`](public-api.md), [`conformance.md`](conformance.md), [`release-1.0.md`](release-1.0.md) |

## 10. 0.1 evaluation baseline and deferred-option triage

Measured 2026-10-29 on the shared Linux x86_64 development host with a release build
(`cargo build --release -p witforge-cli`). Timings are wall-clock time for the whole process,
including startup; the host is shared, so treat them as an order-of-magnitude baseline rather
than a benchmark. Each alint figure is the range of five runs; each versions-root figure is the
range of three runs.

| Fixture | Emitter | Runs | Wall time | Output |
|---|---|---:|---:|---:|
| `contracts/alint-plugin` as `v0.1` | `rust` | 5 | 3–7 ms (median 3) | 7.7 KB, 2 files |
| `contracts/alint-plugin` as `v0.1` | `proto` | 5 | 3 ms | 6.6 KB, 1 file |
| `contracts/alint-plugin` as `v0.1` | `glue` | 5 | 3 ms | 45.6 KB, 2 files |
| `fixtures/versions/wit` (`v0.1`, `v0.2`) | `rust` | 3 | 13 ms | 0.8 KB, 3 files |
| `fixtures/versions/wit` (`v0.1`, `v0.2`) | `proto` | 3 | 8–10 ms | 0.4 KB, 1 file |
| `fixtures/versions/wit` (`v0.1`, `v0.2`) | `glue` | 3 | 5–15 ms | 3.5 KB, 2 files |

The alint end-to-end test, which also runs `prost-build`, `pbjson-build`, and `rustc`, completes in
roughly 15–40 seconds on the same host. That is test-toolchain cost, not generation cost.

Ergonomics observations:

- Generation is four explicit commands (`rust`, `proto`, `schema`, `glue`), because outputs have
  separate roots and lifecycle; `witforge.toml` removes repeated roots and output paths. `glue` requires both module roots; consumers cannot accidentally get a
  guessed dependency.
- Generated plain Rust and glue have no runtime dependency of their own. Protobuf and JSON
  encoding stay on the consumer's `prost`/`pbjson` side.
- Function helpers have zero/one/many arity and a `METHOD` constant; the alint adapter and
  analyzer APIs compile and round-trip without adapter-specific glue.
- The four `--check` commands are cheap and deterministic; stale/extra generated files are
  reported by the shared output owner.
- The same consumer path can use a `witforge.toml` project (section 11 of
  [`emitters.md`](emitters.md)) or the four explicit invocations.

W5-003 triage: no adversarial finding required a new task or a contract exception. The recorded
cases cover absent required fields, option absence, empty repeated fields, unknown oneofs, and
unknown/zero enum values; the only enum-bearing fixture is the dedicated glue contract because
the representative alint contract intentionally contains no enum.

### 10.1 Borrowed Rust parameter measurement (W6-002)

Measured 2026-11-01 on the shared Linux x86_64 development host with `rustc -O` and the
generated alint plain types. The fixture built 1,000 and 10,000 `AnalyzerInput` rows with nested
`FlatValue` nodes, and 64 KiB and 1 MiB adapter byte buffers. Each figure is the range of three
runs; the shared host makes these order-of-magnitude measurements, not a benchmark.

| Operation | Payload | Time per operation |
|---|---:|---:|
| Clone `AnalyzerInput` | 1,000 rows | 0.58–0.69 ms |
| Clone `AnalyzerInput` | 10,000 rows | 7.1–9.0 ms |
| Clone `AdapterInput` | 64 KiB | 1.2–2.0 µs |
| Clone `AdapterInput` | 1 MiB | 24–26 µs |
| Borrowed read of `AnalyzerInput` | 10,000 rows | below 1 ns (no clone) |

The clone cost is real and scales with row count. It is also a lower bound for the alint bridge,
which maps native rows into the generated WIT-owned rows rather than merely cloning them. A
borrowed generated signature would avoid a clone only for a caller that already holds generated
values; it would not remove the native-to-WIT conversion, and a fully borrowed type family would
add lifetimes and a second generated surface for every nested record/variant/list. Partial
borrowing at the function boundary would be inconsistent and would not help the dominant nested
row path.

Decision: keep owned generated parameters for 0.x and 1.0. Borrowed parameters are not adopted
without a new contract version and a separate implementation task with tests. The trigger is a
consumer path where measured clone/mapping cost dominates the call, or a consumer explicitly
needs borrowed inputs; the alint bridge should be re-measured then. This decision changes no
generated output and no wire format.

### 10.2 Conditional `Eq`/`Hash` and `Default` evaluation (W6-003)

The generated Rust surface currently derives `Debug`, `Clone`, and `PartialEq`. A conditional
`Eq`/`Hash` rule is mechanically feasible: `f32` and `f64` are the only primitive blockers, and
a transitive check through aliases, named structural types, records, variants, flags, lists,
options, and results can decide whether every reachable field supports both. A narrow `Default`
rule is also mechanically feasible for primitives, strings, lists, options, flags, and records
whose fields are themselves defaultable. It is not natural for enums, variants, or results:
WIT has no default annotation, and choosing the first case would silently change semantics.

Neither rule is adopted for 0.x or 1.0. Adding a derive changes the generated public API for an
already-generated contract version, while released per-version modules are frozen; applying it
retroactively would also make two contracts with the same WIT emit different APIs. There is no
consumer request for these bounds today; consumers can wrap generated values and derive what
their own container needs. If a consumer needs map/Hash support or fixture defaults, the change
belongs to a new contract version with an explicit, documented derive policy; `Eq`/`Hash` must
still exclude every type with a transitive float, and `Default` must stay limited to naturally
defaultable shapes. No implementation task is created now. The trigger is a consumer request
plus that version decision; a separate implementation task with golden and compile tests is
created then.

### 10.3 Docs-only patches and memberless inventory (W6-006)

No consumer needs docs-only patch versions or a separate memberless inventory. The supported
version shape remains `v<major>.<minor>` with a package version `<major>.<minor>.0`; a directory
named `v0.1.1` is not a version directory and is ignored by root discovery, so it cannot be used
as a docs-only patch. A docs-only change in a draft version is already accepted: it changes
generated documentation, not wire numbers or the generated type/trait shape. A docs-only change
to a released version is forbidden and is caught by the release-freeze check.

Memberless definitions (aliases, named `list`/`option`/`result`, interfaces, and worlds) have no
lockfile entry and therefore no wire identity. A memberless addition in a new minor is accepted
without touching the lockfile and does appear in generated output; adding an inventory entry
would create source-identity bookkeeping unrelated to the wire and would make semantic changes
look like wire changes. Released-directory freezing plus generated-output `--check` remain the
guards for already-released contracts.

Decision: do not add docs-only patch versions or a memberless inventory for 0.x or 1.0. A
consumer request for docs-only patches needs an explicit new version-directory shape and a
semantic-diff rule; a memberless inventory is reconsidered only if a consumer needs to diff
those definitions independently. Neither relaxation may change wire numbers or generated API
shape in a released version.

Deferred options are assigned as follows. Their owners are all `asamarts`; the trigger is the
condition under which the owning task should be scheduled. This record does not accept any of
these options or change generated output.

| Deferred option | Owning task | Trigger |
|---|---|---|
| Declaration-order fingerprinting | `W6-001` | Done (2026-11-01): additive per-parent order fingerprints in the lockfile |
| Borrowed Rust parameters | `W6-002` | Done (2026-11-01): owned retained for 0.x/1.0; revisit only with a new contract version when a measured consumer path is dominated by clone/mapping cost |
| Conditional `Eq`/`Hash` and narrow `Default` | `W6-003` | Done (2026-11-01): declined for 0.x/1.0; requires a consumer request, a new contract version, and an explicit float/default policy |
| Generated world metadata | `W6-004` | Done (2026-11-01): additive `worlds.rs` metadata with per-version imports/exports |
| Configuration file for roots/emitters | `W6-005` | Done (2026-11-01): explicit `witforge.toml` projects, CLI flags override |
| Docs-only patch versions and memberless inventory | `W6-006` | Done (2026-11-01): declined for 0.x/1.0; drafts accept docs-only changes, memberless additions have no wire entry, released docs are freeze-guarded |
| Generated version-to-version conversions | `W6-007` | Done (2026-11-01): `rust` emits `convert.rs` for adjacent versions |
| JSON Schema emitter | `W6-008` | Done (2026-11-01): Draft 7 schema per version |
| Multi-platform CI and public release | `W5-005` | Done (2026-10-31): hosted CI, the public repository, and 0.1.0 on crates.io, tagged `v0.1.0` |
