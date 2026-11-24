---
layout: default
title: Migrating between witforge releases
---

# Migrating between witforge releases

## General upgrade steps

1. Check the release row below and read its full changelog section before moving your pin.
2. Update your installed CLI (`cargo install witforge-cli --version X.Y.Z --locked`) and CI Action
   pin (`asamarts/witforge@vX.Y.Z`) together.
3. Run `witforge check` across every version directory to surface any newly refused invalid or
   ambiguous contracts.
4. Run `witforge generate --diff` (or each generator with `--diff`) to inspect generated-output
   changes, regenerate committed outputs, compile and test your consumer crate, and verify
   `witforge generate --check` passes before committing.

## Per-release output and compatibility summary

Each row links directly to the authoritative release notes in
[`CHANGELOG.md`](https://github.com/asamarts/witforge/blob/main/CHANGELOG.md) rather than copying
its details here.

| Release | Generated output changes for existing contracts? | Summary & changelog link |
|---|---|---|
| **Unreleased** | No announced changes | See [Unreleased in `CHANGELOG.md`](https://github.com/asamarts/witforge/blob/main/CHANGELOG.md#unreleased). |
| **0.8.0** (2026-11-24) | **Minor (JSON Schema docs, proto wrappers/unused imports, long-line Rust/glue formatting)** | Strictly additive Rust library release over 0.7.0; adds lifecycle CLI (`init`, `retire`, `new-version --from`/`--released`), versioned JSON contract IR (`inspect`), opt-in `no_std` + `alloc` Rust/convert/glue output, optional `witforge-build` crate, JSON Schema Draft 2020-12 and member descriptions, static `musl` Linux archives, and Action `check: true` / `generators: all` / default `github-token` — see [0.8.0 in `CHANGELOG.md`](https://github.com/asamarts/witforge/blob/main/CHANGELOG.md#080---2026-11-24). |
| **0.7.0** (2026-11-23) | **Yes for contracts using `@since`, `@unstable`, or `@deprecated`** | Final pre-1.0 Rust-library source break (`#[non_exhaustive]` data types, private former internals, structured workflow/generated findings, typed diff elements and `stability-changed`); stability docs and deprecation markers in Rust/protobuf/JSON Schema; cross-major partial `convert.rs`, per-major `deps/` scoping, retirement, decode diffing, workflow commands, diagnostics, and Action hardening. Regenerate every annotated contract and apply the [0.7.0 migration notes](https://github.com/asamarts/witforge/blob/main/CHANGELOG.md#070---2026-11-23). |
| **0.6.0** (2026-11-21) | **Yes (all consumers)** | Per-generator/contract ownership headers, `rustfmt`/Clippy/`missing_docs`-clean Rust, upper-case Rust identifier folding, dropped empty traits for type-only interfaces, `ConversionError` `from`/`to` fields, lossless JSON Schema `result` + `f32` bounds, byte-list aliases -> `bytes`/`Vec<u8>`, leading-dot cross-package `.proto` refs, deterministic multi-file order, absolute glue module roots, Action `diff-*` activation, and stricter `check`/lockfile rules — read [0.6.0 Migration in `CHANGELOG.md`](https://github.com/asamarts/witforge/blob/main/CHANGELOG.md#060---2026-11-21). |
| **0.5.0** (2026-11-17) | Minor (`convert.rs` trailing newline only) | Removed extra blank line at EOF of `convert.rs`; additive `diff` conversion-impact notes; no wire, proto, schema, or Rust API changes — see [0.5.0 in `CHANGELOG.md`](https://github.com/asamarts/witforge/blob/main/CHANGELOG.md#050---2026-11-17). |
| **0.4.4** (2026-11-12) | Minor (glue & `convert.rs` docs) | Reduced clones/allocations in generated glue and added `# Errors` docs to `convert.rs`; regenerate committed glue and `convert.rs` — see [0.4.4 in `CHANGELOG.md`](https://github.com/asamarts/witforge/blob/main/CHANGELOG.md#044---2026-11-12). |
| **0.4.3** (2026-11-11) | No | Tightened transitive dependency validation for exposed unsupported dependency types — see [0.4.3 in `CHANGELOG.md`](https://github.com/asamarts/witforge/blob/main/CHANGELOG.md#043---2026-11-11). |
| **0.4.2** (2026-11-10) | No | Node 24 Action runtime and stricter world/dependency validation — see [0.4.2 in `CHANGELOG.md`](https://github.com/asamarts/witforge/blob/main/CHANGELOG.md#042---2026-11-10). |
| **0.4.1** (2026-11-06) | Only nested dependency wrappers | Fixed PascalCase synthesized wrapper names for nested `deps/` lists/options — see [0.4.1 in `CHANGELOG.md`](https://github.com/asamarts/witforge/blob/main/CHANGELOG.md#041---2026-11-06). |
| **0.4.0** (2026-11-06) | Only when using `deps/` or `serde = true` | Added standard `deps/` dependency support and opt-in Serde derives — see [0.4.0 in `CHANGELOG.md`](https://github.com/asamarts/witforge/blob/main/CHANGELOG.md#040---2026-11-06). |
| **0.3.0** (2026-11-06) | No | Added `witforge diff`, `--format json|sarif`, prebuilt archives, and GitHub Action — see [0.3.0 in `CHANGELOG.md`](https://github.com/asamarts/witforge/blob/main/CHANGELOG.md#030---2026-11-06). |
| **0.2.0** (2026-11-04) | **Yes (multi-version, worlds, schema)** | Added `convert.rs`, `worlds.rs`, `witforge schema`, `witforge.toml`, and `# witforge order v1` lockfile fingerprints — see [0.2.0 in `CHANGELOG.md`](https://github.com/asamarts/witforge/blob/main/CHANGELOG.md#020---2026-11-04). |
| **0.1.2** (2026-11-01) | No | Validation and CRLF/lockfile/freeze hardening only — see [0.1.2 in `CHANGELOG.md`](https://github.com/asamarts/witforge/blob/main/CHANGELOG.md#012---2026-11-01). |
| **0.1.1** (2026-11-01) | No | Output ownership, symlink refusal, and release-freeze hardening — see [0.1.1 in `CHANGELOG.md`](https://github.com/asamarts/witforge/blob/main/CHANGELOG.md#011---2026-11-01). |
| **0.1.0** (2026-10-31) | Initial release | Initial public compiler release (`check`, `lock`, `rust`, `proto`, `glue`) — see [0.1.0 in `CHANGELOG.md`](https://github.com/asamarts/witforge/blob/main/CHANGELOG.md#010---2026-10-31). |
