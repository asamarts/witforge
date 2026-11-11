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

For compatible owned value types, Rust `wit_bindgen::generate!` can use `with` mappings to reuse witforge-generated types directly. This may avoid a second application conversion. It still performs required Canonical ABI memory work and couples the integration to wit-bindgen's generated API.

## Host adapters

Wasmtime host bindings include runtime traits and linker machinery. A plain witforge module is not a replacement. Keep an application adapter between host-generated values and domain contract values, and measure nested-list allocations before proposing generated bridge code.

Use witforge when the same WIT values also need native Rust, protobuf/process, schema, or version-evolution surfaces. Otherwise use wit-bindgen/Wasmtime directly.
