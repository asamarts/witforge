# Polyglot Emitters Specification (`W13-001`)

Status: normative design specification for built-in polyglot emitters (`M-p13`, `W13-001`..`W13-015`)
and shared nested-tree output conventions reused by third-party plugins (`W11-001`).

## 1. Goals and constraints

witforge keeps one canonical Rust `witforge` binary and library crate (`crates/witforge` depends
only on `wit-parser` and `anyhow`). Built-in polyglot emitters ship as strictly additive `0.x`
modules alongside the existing Rust, protobuf, JSON Schema, Rust glue, and Markdown doc emitters:

- **Zero regressions to existing outputs:** Rust, protobuf, JSON Schema, Rust glue, Markdown doc,
  `wire.lock`, and `witforge check` remain byte-for-byte unchanged for existing contracts.
- **Target-agnostic `witforge check`:** Default `witforge check` never rejects valid `0.9.0` WIT
  contracts for target-language keywords; every polyglot emitter escapes target keywords
  deterministically (section 5).
- **Zero-dependency plain models:** Every plain emitter produces clean, idiomatic, modern-LTS
  code with zero third-party runtime dependencies.
- **Opt-in checked protobuf glue:** Every language pairs its plain emitter with an opt-in glue
  emitter targeting that ecosystem's official standard protobuf runtime, enforcing all 13 rows of
  the WIT decode profile (`docs/guide/non-rust.md`) plus encode-side narrow/unsigned integer,
  Unicode scalar `char`, and fixed-length list bounds checks.
- **Distinct output roots:** Plain and glue outputs always live in separate, non-overlapping
  output roots validated by `ensure_distinct_output_roots`, so every directory tree has a single
  unambiguous ownership header.

## 2. Target baselines and protobuf runtimes

| Wave | Language | Plain baseline (zero runtime deps) | Checked protobuf glue runtime |
|---|---|---|---|
| Wave 1 | TypeScript | TypeScript 5.0+ / 6.0+ strict ESM (`strict: true`, explicit `.js` imports) | `@bufbuild/protobuf` (`protobuf-es` v2) |
| Wave 1 | Python | Python 3.11+ stdlib (`@dataclass(frozen=True, slots=True)`, `Enum`, `Protocol`, `py.typed`) | `google.protobuf` (`protoc --python_out` + `json_format`) |
| Wave 1 | Go | Go 1.22+ stdlib (`struct`, typed `enum` + `Valid()`, sealed variant/result interfaces, `[N]T`) | `google.golang.org/protobuf` (`protoc-gen-go` + `protojson`) |
| Wave 2 | Java | Java 21 LTS+ stdlib (`record`, `sealed interface ... permits ...`, `enum`, `Optional`) | `com.google.protobuf` (`protobuf-java` + `protobuf-java-util` `JsonFormat`) |
| Wave 2 | C# | C# 12+ / .NET 8 LTS+ stdlib (`#nullable enable`, `sealed record`, `System.Text.Rune`) | `Google.Protobuf` (`JsonFormatter` / `JsonParser`) |

## 3. CLI subcommands and `witforge.toml` configuration

| Language | Plain CLI | Plain TOML section | Glue CLI | Glue TOML section |
|---|---|---|---|---|
| TypeScript | `witforge ts` | `[project.<name>.typescript]` | `witforge ts-glue` | `[project.<name>.typescript-glue]` |
| Python | `witforge py` | `[project.<name>.python]` | `witforge py-glue` | `[project.<name>.python-glue]` |
| Go | `witforge go` | `[project.<name>.go]` | `witforge go-glue` | `[project.<name>.go-glue]` |
| Java | `witforge java` | `[project.<name>.java]` | `witforge java-glue` | `[project.<name>.java-glue]` |
| C# | `witforge csharp` | `[project.<name>.csharp]` | `witforge csharp-glue` | `[project.<name>.csharp-glue]` |

- Every emitter command supports `[WIT_ROOT]`, `--out <DIR>`, `--check`, `--diff`,
  `--released <PATH>`, `--config <PATH>` / `--no-config`, and `--project <NAME>`.
- Every configured section requires `out` and participates in `ensure_distinct_output_roots` and
  `witforge generate`.
- `witforge generate` supports `--only <GEN[,GEN...]>` (`W13-014`) to run a validated subset of
  configured generators (`rust`, `proto`, `schema`, `glue`, `doc`, `ts`, `ts-glue`, `py`,
  `py-glue`, `go`, `go-glue`, `java`, `java-glue`, `csharp`, `csharp-glue`, `plugins`).
- Configured third-party plugins (`W11-001`) are never executed by `witforge generate` unless
  `--allow-plugins` is explicitly passed on the CLI.

## 4. Directory layout, versions, and `DepLayout`

All five polyglot emitters emit deterministic, LF-only nested directory trees managed through
`witforge::emit::output::GeneratedFiles::new_nested`:

- **Plain emitters** emit one per-version subtree per live version (`v<major>_<minor>/`), plus
  shared or per-major dependency subtrees under `witforge_deps/`, `worlds` metadata when any
  version declares a world, and `convert` adapters when two or more live versions exist
  (`W13-012` / `W13-013`).
- **Checked protobuf glue emitters** emit one per-major subtree (`v<major>/`) for the highest
  minor of each live major, plus a shared `GlueError` / `WitforgeGlueException` module and
  dependency glue subtrees mirroring `DepLayout`.

### 4.1 `DepLayout` mapping (`shared`, `per_major`, `disambiguated`)

`crates/witforge/src/emit/versions.rs` (`DepLayout`) classifies every `deps/` dependency across
all live majors:

1. **Shared (`root_major = None`):** Identical across every live major that uses it ->
   `witforge_deps/<ns>_<pkg>/` (`polyglot::dep_dir_path(None, ns, name)`).
2. **Per-major scoped (`root_major = Some(m)`):** Upgraded or changed across live majors ->
   `witforge_deps/v<m>/<ns>_<pkg>/` (`polyglot::dep_dir_path(Some(m), ns, name)`). When
   `canonical_owner < m`, emitters re-export or alias the canonical owner's types so adjacent
   majors with identical pinned dependencies share one type identity.
3. **Disambiguated protobuf packages (`DepLayout::is_disambiguated`):** When two live majors
   pin different definitions of the same dependency major, protobuf packages use
   `<ns>.<pkg>.v<dep_major>_m<owner>` (`polyglot::disambiguated_major_tag(dep_major, Some(owner))`).
   Glue emitters derive protobuf imports/namespaces from that exact disambiguated package name so
   they align with `witforge proto` (`W11-039`).

### 4.2 Per-language directory layouts

- **TypeScript (`witforge ts`, `witforge ts-glue`):**
  - `index.ts`, `v<major>_<minor>/<interface>.ts`, `v<major>_<minor>/index.ts`,
    `witforge_deps/<ns>_<pkg>/<interface>.ts` (or `witforge_deps/v<m>/<ns>_<pkg>/<interface>.ts`),
    `worlds.ts`, `convert.ts`.
  - Relative ESM specifiers always carry explicit `.js` extensions (`./v0_1/index.js`) so code
    runs under NodeNext, bundler, Deno, and Bun without extension rewrites.
- **Python (`witforge py`, `witforge py-glue`):**
  - `py.typed` (empty PEP 561 marker registered via `GeneratedFiles::insert_marker`),
    `__init__.py`, `v<major>_<minor>/__init__.py`, `v<major>_<minor>/<interface>.py`,
    `witforge_deps/__init__.py`, `witforge_deps/<ns>_<pkg>/__init__.py`, `worlds.py`,
    `convert.py`.
- **Go (`witforge go`, `witforge go-glue`):**
  - One Go package per directory: `v<major>_<minor>/<interface>/<interface>.go` (or per-version
    package `v<major>_<minor>/<interface>.go` with interface-prefixed exported types when sharing
    one Go package per version), `witforge_deps/<ns>_<pkg>/...`, `worlds/worlds.go`,
    `convert/convert.go`. Exact import path prefix is configured via `--go-module` / `go-module`.
- **Java (`witforge java`, `witforge java-glue`):**
  - Standard nested package directory matching `--java-package` / `java-package`, e.g.
    `<pkg_path>/v<major>_<minor>/<interface>/<Type>.java`, one public top-level type per `.java`
    file, plus `witforge_deps/...`, `Worlds.java`, and `Convert.java`.
- **C# (`witforge csharp`, `witforge csharp-glue`):**
  - `<Namespace>/V<major>_<minor>/<Interface>.cs`, `WitforgeDeps/<NsPkg>/<Interface>.cs`,
    `Worlds.cs`, `Convert.cs`, with `#nullable enable` on line 2 right after the ownership header.

## 5. Ownership headers, markers, and nested `output.rs` safety

`crates/witforge/src/emit/output.rs` provides the single canonical nested-tree engine shared by
all polyglot emitters and `W11-001` third-party plugins:

### 5.1 Ownership header styles (`HeaderStyle`)

| Style | Format | Generators |
|---|---|---|
| `HeaderStyle::LineComment` | `// @generated by witforge (<gen>, <ns>:<name>); do not edit.` | `rust`, `proto`, `glue`, `ts`, `ts-glue`, `java`, `java-glue`, `csharp`, `csharp-glue` |
| `HeaderStyle::GoGenerated` | `// Code generated by witforge (<gen>, <ns>:<name>); DO NOT EDIT.` | `go`, `go-glue` |
| `HeaderStyle::HashComment` | `# @generated by witforge (<gen>, <ns>:<name>); do not edit.` | `py`, `py-glue` |
| `HeaderStyle::JsonComment` | `{"$comment":"@generated by witforge (<gen>, <ns>:<name>); do not edit.",` | `schema` |
| `HeaderStyle::HtmlComment` | `<!-- @generated by witforge (<gen>, <ns>:<name>); do not edit. -->` | `doc` |

`HeaderStyle::GoGenerated` conforms to Go's official generated-file convention
(`^// Code generated .* DO NOT EDIT\.$`) recognized by `go doc`, `pkg.go.dev`, and `staticcheck`.
Header classification detects every style across all generators: placing a Python file in a Rust
output root or a Go file in a TypeScript output root is refused as a foreign owner (`WF9004`,
exit `2`).

### 5.2 Marker files (`GeneratedFiles::insert_marker`)

Files whose external specification prohibits a comment header (such as empty PEP 561 `py.typed`
marker files) are registered with `GeneratedFiles::insert_marker`. On disk, a marker path is
considered owned if it is empty or matches the expected marker bytes verbatim; if it carries a
foreign witforge header it is refused as foreign (`WF9004`), and if it contains non-matching
hand-written bytes it is classified as `Unowned` and never overwritten.

### 5.3 Cross-platform path and symlink guards

Every nested path registered via `GeneratedFiles::insert_relative` or `insert_marker` is
validated by `validate_relative_path`:

- Must be a non-empty relative POSIX path (`/` separator only; `\` is rejected).
- Must not start or end with `/`, contain `//`, contain `.` or `..` segments, or contain NUL or
  ASCII control characters (`< 0x20` or `0x7F`).
- Must not use a witforge temporary filename (`.witforge.<name>.<pid>.<seq>.tmp`), end any
  segment with a trailing space or `.`, contain Windows-illegal characters (`< > : " | ? *`), or
  use a Windows reserved device name (`CON`, `PRN`, `AUX`, `NUL`, `COM1`..`COM9`, `LPT1`..`LPT9`,
  case-insensitively with or without extension).
- Duplicate exact paths, ASCII case-insensitive path collisions (`Foo.ts` vs `foo.ts`), and
  case-insensitive file-vs-directory prefix collisions (`a/b` vs `a/b/c`) within one output set
  are refused before writing.
- Recursive walks never descend into symlinked directories. Every ancestor directory and leaf
  path of every expected file is inspected via `symlink_metadata` before writing: symlinked
  ancestors, non-directory ancestors, symlinked leaves, non-regular leaves, foreign-owned files,
  and unowned files at expected paths are refused with `WF9004` (`write`) or reported as findings
  (`check`).
- After atomic writes succeed, stale owned files are removed and empty subdirectories strictly
  inside `out` are pruned upward without touching `out` itself or any directory containing
  unowned files.

## 6. Identifier conventions, escaping, docs, and stability

Shared helpers live in `crates/witforge/src/emit/polyglot.rs`:

| Target | Types / Enums | Fields / Params / Methods | Escaping rule | Doc comments | Deprecation |
|---|---|---|---|---|---|
| TypeScript | `PascalCase` | `camelCase` | Trailing `_` for ES/TS keywords (`type_`, `class_`) | `/** ... */` JSDoc/TSDoc | `@deprecated <note>` JSDoc tag |
| Python | `PascalCase` | `snake_case` (`UPPER_SNAKE` enum members) | Trailing `_` for Python 3.11+ hard/soft keywords (`class_`, `match_`) | Triple-quoted PEP 257 docstrings | Docstring note + optional `warnings.warn` / `@deprecated` comment |
| Go | Exported `PascalCase` | Exported `PascalCase` fields/methods; lowercase params/packages | Trailing `_` for lowercase Go keywords, `_`, and predeclared universe names (`string_`, `error_`) | `//` Go doc comments | `// Deprecated: <note>` Go doc convention |
| Java | `PascalCase` | `camelCase` (`UPPER_SNAKE` enum constants) | Trailing `_` for JLS keywords, restricted `TypeIdentifier`s (`record_`, `var_`), and `Object` method clashes on record components | `/** ... */` Javadoc | `@Deprecated` annotation + `@deprecated <note>` Javadoc tag |
| C# | `PascalCase` | `PascalCase` properties/methods; `camelCase` params | `@` verbatim prefix for C# keywords (`@class`, `@event`); trailing `_` for `System.Object` / `record` member clashes (`EqualityContract_`, `ToString_`) | `/// <summary>...</summary>` XML docs | `[global::System.Obsolete("...")]` |

Because WIT identifiers cannot contain underscores, trailing `_` (and C# `@`) escapes can never
collide with another WIT identifier. All doc strings are normalized first through
`polyglot::doc_lines` (stripping C0/C1 control characters and decorative `/** *` gutters).

## 7. WIT-to-language type mapping tables

### 7.1 Primitives and structural types

| WIT | TypeScript | Python 3.11+ | Go 1.22+ | Java 21+ | C# 12+ / .NET 8+ |
|---|---|---|---|---|---|
| `bool` | `boolean` | `bool` | `bool` | `boolean` / `Boolean` | `bool` |
| `u8` | `number` | `int` | `uint8` | `int` (unsigned 0..255) | `byte` |
| `u16` | `number` | `int` | `uint16` | `int` (unsigned 0..65535) | `ushort` |
| `u32` | `number` | `int` | `uint32` | `int` (unsigned 32-bit) | `uint` |
| `u64` | `bigint` | `int` | `uint64` | `long` (unsigned 64-bit) | `ulong` |
| `s8` | `number` | `int` | `int8` | `byte` | `sbyte` |
| `s16` | `number` | `int` | `int16` | `short` | `short` |
| `s32` | `number` | `int` | `int32` | `int` | `int` |
| `s64` | `bigint` | `int` | `int64` | `long` | `long` |
| `f32` | `number` | `float` | `float32` | `float` / `Float` | `float` |
| `f64` | `number` | `float` | `float64` | `double` / `Double` | `double` |
| `char` | `string` (1 Unicode scalar) | `str` (1 Unicode scalar) | `rune` | `int` (Unicode code point) | `global::System.Text.Rune` |
| `string` | `string` | `str` | `string` | `String` | `string` |
| `list<u8>` | `Uint8Array` | `bytes` | `[]byte` | `byte[]` (defensively copied) | `global::System.ReadOnlyMemory<byte>` / `byte[]` |
| `list<T>` | `readonly T[]` | `tuple[T, ...]` | `[]T` | `List<T>` (`List.copyOf`) | `IReadOnlyList<T>` |
| `list<u8, N>` | `Uint8Array` (length `N` checked) | `bytes` (length `N` checked) | `[N]byte` | `byte[]` (length `N` checked) | `byte[]` / `ReadOnlyMemory<byte>` (length `N` checked) |
| `list<T, N>` | `readonly T[]` (tuple for small `N` or length-checked array) | `tuple[T, ...]` (length `N` checked) | `[N]T` | `List<T>` (length `N` checked) | `IReadOnlyList<T>` (length `N` checked) |
| `map<K, V>` | `ReadonlyMap<K, V>` | `Mapping[K, V]` / `dict[K, V]` | `map[K]V` | `Map<K, V>` (`Map.copyOf`) | `IReadOnlyDictionary<K, V>` |
| `option<T>` | `Option<T>` (`{ readonly tag: "none" } \| { readonly tag: "some"; readonly value: T }`) | `T \| None` (or `Option[T]` wrapper when nested `option<option<T>>`) | `*T` (or generic `Option[T]` struct for unambiguous nesting) | `Optional<T>` | `T?` (or `WitforgeOption<T>` struct for unambiguous nesting) |
| `result<T, E>` | `{ readonly tag: "ok"; readonly value: T } \| { readonly tag: "err"; readonly error: E }` | `Ok[T] \| Err[E]` frozen dataclasses | `Result[T, E]` struct / sealed interface | `sealed interface Result<T, E> permits Ok, Err` | `abstract record Result<T, E>` + `sealed record Ok` / `Err` |

Note on `option<option<T>>`: because `null`/`undefined` in TS, `None` in Python, `nil` in Go,
`Optional` in Java, and `T?` in C# collapse nested absence unless wrapped, every language emitter
preserves `Some(None)` distinct from `None` whenever `option<option<T>>` occurs (either using an
explicit tagged `Option<T>` uniformly or wrapping nested options deterministically).

### 7.2 Nominal declarations, interfaces, and worlds

| WIT | TypeScript | Python 3.11+ | Go 1.22+ | Java 21+ | C# 12+ / .NET 8+ |
|---|---|---|---|---|---|
| `record` | `export interface` with `readonly` properties | `@dataclass(frozen=True, slots=True)` | `type Name struct { ... }` | `public record Name(...)` | `public sealed record Name(...)` |
| `variant` | Discriminated union on `readonly tag: "..."` | `Union` of `@dataclass(frozen=True, slots=True)` case classes | Sealed interface with unexported marker method + case structs | `public sealed interface Name permits ...` + case `record`s | `public abstract record Name` (private ctor) + nested `public sealed record` cases |
| `enum` | String union type + `const` object of values | `class Name(str, Enum)` | `type Name uint32` + `const` values + `Valid() bool` | `public enum Name` | `public enum Name` |
| `flags` | `export interface` of `readonly <flag>: boolean` | `@dataclass(frozen=True, slots=True)` of `bool` fields | `type Name struct` of `bool` fields | `public record Name(...)` of `boolean` fields | `public sealed record Name(...)` of `bool` properties |
| `type a = b` / `use ... as` | `export type A = B;` | `type A = B` / `TypeAlias` | `type A = B` | Typed wrapper / import alias (Java has no type aliases; inlines to resolved target type matching protobuf/Javadoc) | `global using` or resolved target type |
| `interface` (with functions) | `export interface Name { ... }` | `class Name(Protocol): ...` | `type Name interface { ... }` | `public interface Name { ... }` | `public interface I<Name> { ... }` |
| `world` metadata | `export const WORLDS = ... as const;` | Frozen `WorldMeta` constants | `Worlds` metadata structs/slices | `public final class Worlds` constants | `public static class Worlds` constants |

## 8. Checked protobuf glue and `GlueError` shapes

Every language glue emitter enforces all 13 rows of the WIT decode profile
(`docs/guide/non-rust.md`) on decode AND validates narrow/unsigned integer bounds, Unicode scalar
`char` values (rejecting surrogates `0xD800..=0xDFFF` and values `> 0x10FFFF`), and exact
`list<T, N>` lengths on encode (for languages whose native type is wider than the WIT type, such
as TypeScript `number`/`bigint`, Python `int`/`str`, Java unsigned `int`/`long` and `int` code
points, and Go `rune`).

All five languages expose the exact same seven error kinds matching Rust's `GlueError`:

| Error kind | Fields | Trigger |
|---|---|---|
| `IntegerOutOfRange` | `field`, `value` | `u8`/`u16`/`s8`/`s16` (or encode-side `u32`/`u64`/`s32`/`s64`) outside WIT range |
| `MissingField` | `field` | Absent required `optional` scalar/string/bytes/enum or message field |
| `UnknownEnumValue` | `enumeration`, `value` | Zero (`*_UNSPECIFIED`) or unknown enum tag |
| `UnknownCase` | `message` | Unset or unknown `oneof` case on a variant or result |
| `InvalidState` | `message` | Synthesized wrapper (`WitforgeOptionOf*`, `WitforgeResultOf*`) with unset `oneof` |
| `InvalidChar` | `field`, `value` | Non-Unicode-scalar code point (UTF-16 surrogate or `> 0x10FFFF`) |
| `InvalidLength` | `field`, `expected`, `actual` | `list<T, N>` shorter or longer than exact length `N` |

Language-specific error types:

- **TypeScript:** `export class WitforgeGlueError extends Error` with `readonly kind: GlueErrorKind` and typed detail fields.
- **Python:** `class WitforgeGlueError(ValueError)` with `kind: GlueErrorKind` (`Literal[...]`) and detail attributes.
- **Go:** `type GlueError struct` implementing `error` with `Kind GlueErrorKind` and detail fields (`*GlueError`).
- **Java:** `public final class WitforgeGlueException extends Exception` (or runtime/checked per `W13-009` with `GlueErrorKind kind()`).
- **C#:** `public sealed class WitforgeGlueException : Exception` with `GlueErrorKind Kind` and detail properties.

## 9. Cross-version conversion adapters (`W13-012` / `W13-013`)

`W13-012` extracts the language-neutral convertibility analysis from `crates/witforge/src/emit/convert.rs`
so Rust, TypeScript, Python, Go, Java, and C# all apply identical evolution rules between
adjacent live versions:

- Added `option<T>`, `list<T>`, and `map<K, V>` fields default on upgrade (`None` / empty);
- Removed required members, narrowed types, and unknown enum/variant cases return structured
  conversion errors on downgrade;
- Pinned `deps/` sharing the same `DepLayout` canonical owner copy directly across adjacent
  majors;
- Non-convertible types across major boundaries are omitted with deterministic omission notes.

## 10. Two-tier verification contract

Every built-in language emitter (`W13-002`..`W13-013`) must satisfy both verification tiers:

1. **Tier 1 (always offline in `cargo test --workspace --locked`):** Deterministic golden tests,
   edge-case tests (keyword escaping, uppercase/digit names, nested wrappers, `list<T, N>`,
   `map<K, V>`, `char`, multi-major + disambiguated `deps/`, stability/deprecation, `--check`
   drift), and negative error-path tests run on Linux, macOS, Windows, and MSRV `1.88` with zero
   external toolchains required.
2. **Tier 2 (incremental SHA-pinned CI jobs in `.github/workflows/ci.yml`):** Each language glue
   task (`W13-003`, `W13-005`, `W13-007`, `W13-009`, `W13-011`) adds an incremental CI job using
   SHA-pinned official setup actions and locked tool/package versions that compiles emitted plain
   code + protobuf schemas + checked glue and executes every `W11-034` binary and proto3-JSON
   conformance vector against the Rust reference oracle.
