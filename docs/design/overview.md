# witforge design overview

Status: living design document (2026-10-14; decisions through 2026-10-26 are logged in section
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
later JSON Schema. It does not generate WASM guest or host bindings (`wit-bindgen` and
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
  -> emitters: Rust, protobuf, prost glue,          emit (0.1)
     later JSON Schema
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
semver-compatible by adding modules; the old-to-new conversions of section 5.3 connect them. The
concrete layout, naming, derives, and trait conventions are in [`emitters.md`](emitters.md).

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

Function method ids and parameters are specified (decided 2026-10-24, format fixed 2026-10-25);
the implementation covers type members so far. The v2 format and the rules in this section are
implemented by the lockfile v2 roadmap slice; the current code still emits the v1 format.

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
  with every entry treated as a draft;
- `wit/released` lists released version directories; their version directories are frozen
  (shared `mod.rs`, proto, and glue output advances append-only as described in section 5.3).
  `lock` refuses them, and CI fails if anything in a released version directory changes against
  the merge base. A new minor is developed with the previous version as its `--against` baseline;
- CI fails if a member is unlocked, if a released directory changes, or if generated output is
  stale; `buf breaking` on the emitted schema is an optional extra check;
- the lockfile is designed to be replaced by in-source numbers if the WIT annotations proposal
  (component-model issue 695) lands with a suitable annotation.

### 5.3 Evolution policy

1. A published contract version is frozen. `wit/released` lists the released version
   directories; their WIT, lockfile, and per-version generated Rust module are immutable. The
   shared `mod.rs`, per-major proto schema, and glue output may change when a new minor is added;
   their released members are protected by the lockfile and `--check`, and the proto schema is
   append-only.
2. Changes ship as a new minor version (`0.x` while unstable), with the previous WIT kept in the
   tree (`wit/v0.1/`, `wit/v0.2/`, and so on), each with its own `wire.lock` (section 5.2). A new
   major version starts a fresh lockfile. Package versions are `<major>.<minor>.0`; patch,
   pre-release, and build-metadata contract versions are out of scope for 0.1 (see
   [`lockfile.md`](lockfile.md)).
3. New members must be defaulted (`option<T>` or `list<T>`), so the compiler can generate the
   old-to-new conversions; enum values and variant cases are append-only; reorders, renames, and
   type changes are forbidden. The 0.1 lockfile does not track declaration order: released
   directories freeze it, and a pre-golive task evaluates an order fingerprint.
4. The host keeps one adapter per supported contract version and converts each into the latest
   plain types; the supported range is published.
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
  and glue output; at least one generated Rust/glue fixture is compiled and one proto schema is
  parsed.
- Round-trip tests: generated Rust values encoded to protobuf and proto3-JSON and decoded back;
  glue conversions and per-function request/response helpers are exercised through the same path.
- Lockfile property tests with `proptest`: reconcile is idempotent, never renumbers, and the
  append-only/draft rules hold across random sequences of member additions, removals, renames,
  and type changes. The cases are listed in [`lockfile.md`](lockfile.md#14-testing-requirements).
- CLI integration tests with `assert_cmd` and `tempfile` for `check`, `lock`, `--against`,
  `--first`, `--released`, `rust`, `proto`, `glue`, and `--check`.
- Subset tests: every refused construct has a fixture (`tests/fixtures/refused/<construct>/`)
  with the exact violations it must produce.
- CI: MSRV 1.88; `cargo fmt`, pedantic Clippy, and rustdoc with warnings denied. Linux only (a
  self-hosted runner, `ci/README.md`) while the repository is private; Linux, macOS, and Windows
  on GitHub-hosted runners once it is public.

## 8. Relationship to alint

alint consumes witforge to generate its `alint-plugin-api` crate and the `.proto` schema its
out-of-process plugin protocol uses, and the conversions between the two (section 4.2). alint
keeps the host glue: wasmtime bindings, the process protocol's framing and session, and the
version adapters. The repository stays private until alint's contract `0.x` consumes
witforge, then goes public with 0.1.

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
| 2026-10-25 | Declaration order is not tracked in the MVP; a pre-golive task evaluates the order-fingerprint option | section 5.2, `ROADMAP.md` |
| 2026-10-25 | `@unstable` and `@since` are metadata; every item in a version directory is part of that version | [`lockfile.md`](lockfile.md) |
| 2026-10-25 | Functions get per-function request/response messages; the response always has `result = 1`; result oneofs use `Empty` for missing payloads | [`emitters.md`](emitters.md) |
| 2026-10-25 | Generated Rust groups types into per-interface submodules, re-exports interface traits at the version root, and uses `&self` with owned parameters/returns and no extra bounds | [`emitters.md`](emitters.md) |
| 2026-10-25 | Generated Rust derives `Debug`, `Clone`, and `PartialEq`; `Eq`, `Hash`, and `Default` are deferred | [`emitters.md`](emitters.md) |
| 2026-10-25 | Rust name conversion uses raw identifiers where possible, a trailing `_` for `self`/`Self`/`crate`/`super`, fully qualified `::std` paths, and hard errors on collisions | [`emitters.md`](emitters.md) |
| 2026-10-25 | Protobuf emits one file per major, a flat interface-prefixed package, a shared `Empty`, and inlines named list/option/result definitions | [`emitters.md`](emitters.md) |
| 2026-10-25 | Anonymous protobuf wrappers use deduplicated structural `Witforge...` names; enum and method values are prefixed with `UNSPECIFIED = 0`; reserved members emit number and proto name | [`emitters.md`](emitters.md) |
| 2026-10-25 | Named `option`/`list` types keep their name for the direct wrapper form; a differing oneof-singular form uses an anonymous `Witforge...` wrapper | [`emitters.md`](emitters.md) |
| 2026-10-25 | Generation takes the WIT root; `rust` and `proto` are separate subcommands with fixed output layout and header-owned `--check` semantics | [`emitters.md`](emitters.md) |
| 2026-10-25 | Version directories are `v<major>.<minor>`, package versions are `<major>.<minor>.0`, Rust modules are `v<major>_<minor>`, proto packages are `v<major>` | [`lockfile.md`](lockfile.md) |
| 2026-10-25 | CLI uses `clap`; lockfile properties use `proptest`; CLI tests use `assert_cmd` and `tempfile`; golden expectations are plain committed files | section 7, `Cargo.toml` |
| 2026-10-25 | Worlds generate no Rust or protobuf code in the MVP; world metadata is a later option | [`emitters.md`](emitters.md) |
| 2026-10-26 | Glue uses free functions per named type, grouped by interface and type, with infallible `to_proto` and fallible `from_proto`; every named type in the highest minor is covered | [`emitters.md`](emitters.md) |
| 2026-10-26 | Glue covers only the highest minor of each major; older plain modules are for native adapters and later version conversions | [`emitters.md`](emitters.md) |
| 2026-10-26 | Glue has a separate output root with required `--plain-root` and `--proto-root`, one `v<major>.rs` per major, and the same `--check`/ownership rules | [`emitters.md`](emitters.md) |
| 2026-10-26 | Glue generates per-function `to_request`/`from_request`/`to_response`/`from_response` helpers and a `METHOD` constant, with ergonomic zero/one/many arity and functions under a `functions` module | [`emitters.md`](emitters.md) |
| 2026-10-26 | Glue returns one structured, non-exhaustive `GlueError`; JSON and binary encoding stay on the pbjson/prost side | [`emitters.md`](emitters.md) |
| 2026-10-26 | Required scalar/string/bool/float/enum/bytes fields, parameters, and response results use proto3 `optional` and reject absence; `option<T>` permits absence | [`emitters.md`](emitters.md) |
