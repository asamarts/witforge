---
layout: default
title: witforge documentation
---

# witforge

witforge compiles one hand-written WIT package into runtime-independent contract artifacts:

- plain Rust types and interface traits;
- protobuf messages with stable field and method numbers;
- Draft 7 JSON Schema;
- checked Rust/protobuf conversion glue;
- adapters between adjacent contract versions;
- compatibility reports and deterministic CI checks.

It is a contract compiler, not a Wasm runtime, RPC framework, or replacement for wit-bindgen.

1. [Choose whether witforge fits](guide/choosing-witforge.md).
2. Follow the [quickstart](quickstart.md).
3. Learn the [contract layout and supported WIT](guide/contract-authoring.md).
4. Configure [generation](guide/generation.md) and [wire a Rust crate](guide/wiring.md).
5. Inspect [real generated output](guide/generated-output.md).
6. Add [CI checks](guide/ci.md).
7. Review [contract evolution](guide/evolution.md) and [release migrations](guide/migration.md).
8. Look up flags and keys in the [CLI and configuration reference](guide/reference.md).

## Important boundaries

- [JSON projections](guide/json-projections.md)
- [Non-Rust implementation rules](guide/non-rust.md)
- [wit-bindgen, Wasmtime, and witforge](guide/component-model.md)
- [Runnable examples](guide/examples.md)

## Reference

- [Design overview](https://github.com/asamarts/witforge/blob/main/docs/design/overview.md)
- [Emitter specification](https://github.com/asamarts/witforge/blob/main/docs/design/emitters.md)
- [Lockfile specification](https://github.com/asamarts/witforge/blob/main/docs/design/lockfile.md)
- [Diagnostics](https://github.com/asamarts/witforge/blob/main/docs/design/diagnostics.md)
- [GitHub Action](https://github.com/asamarts/witforge/blob/main/docs/design/action.md)
- [Public API policy](https://github.com/asamarts/witforge/blob/main/docs/design/public-api.md)

Current release: **0.7.0**. Rust MSRV: **1.88**. The 1.0 release remains gated on successful independent-consumer validation.
