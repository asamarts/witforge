---
layout: default
title: Component Model boundary
---

# wit-bindgen, Wasmtime, and witforge

These tools consume WIT for different purposes.

| Tool | Owns |
|---|---|
| wit-bindgen | Guest bindings, imports/exports, Canonical ABI lift/lower |
| Wasmtime `component::bindgen!` | Typed host bindings, linking, stores, calls, traps, async execution |
| witforge | Runtime-independent Rust values/traits, protobuf identity, JSON Schema, compatibility, version adapters |

witforge does not generate guest export shims or load components.

## Guest type reuse

Rust `wit_bindgen::generate!` can remap selected WIT types with `with`, but a Rust path match is
not enough: the remapped type must provide every API wit-bindgen's generated Canonical ABI code
uses. Verified with wit-bindgen 0.63.0:

- owned records and payload variants can reuse witforge types when their complete reachable type
  graph contains compatible primitives, strings, lists, options, records, and variants;
- witforge enums cannot be remapped because wit-bindgen calls a generated hidden `_lift`
  constructor;
- witforge flags cannot be remapped because wit-bindgen expects the `bitflags` API: `empty`,
  `from_bits_retain`, and `bits`;
- a remapped record or variant containing one of those incompatible enum or flags types also
  fails, even if the nested type itself is left generated.

Keep enums, flags, and aggregates that contain them wit-bindgen-generated, then convert at the
application boundary. Do not add wit-bindgen-specific hidden helpers or a `bitflags`-shaped API to
witforge's runtime-independent types. If an adopter needs direct reuse, the cleaner change belongs
upstream in wit-bindgen. Re-run a compile check when upgrading wit-bindgen because `with` is tied
to its generated API.

Type reuse avoids an application-level conversion for compatible values. It does not avoid the
Canonical ABI's required lift/lower memory work.

## Host adapters

Wasmtime host bindings include runtime traits and linker machinery. A plain witforge module is not a replacement. Keep an application adapter between host-generated values and domain contract values, and measure nested-list allocations before proposing generated bridge code.

Use witforge when the same WIT values also need native Rust, protobuf/process, schema, or version-evolution surfaces. Otherwise use wit-bindgen/Wasmtime directly.
