---
layout: default
title: Choosing witforge
---

# Choosing witforge

## Good fit

witforge fits when:

- hand-written WIT is the contract source of truth;
- the callable contract is pure, synchronous, and value-oriented;
- native Rust and out-of-process implementations share the same semantic values;
- protobuf numbers must remain stable across versions;
- generated output is committed and checked for drift;
- contract changes need machine-readable compatibility review;
- adjacent Rust versions need generated adapters.

Typical examples are analyzers, validators, bounded policy calls, transformation requests, and plugin operations that take and return finite values.

## Use another tool directly

Use [wit-bindgen](https://github.com/bytecodealliance/wit-bindgen) and a Component Model runtime when you only need guest/host bindings and Canonical ABI support.

Use protobuf tooling directly when protobuf is already authoritative and WIT is only a generated view.

Use an application runtime or protocol framework for:

- component loading and linking;
- process framing and multiplexing;
- callbacks, cancellation, and asynchronous streams;
- capability grants, resource limits, retries, and failure policy;
- discovery, signing, packaging, and distribution.

witforge does not own those concerns.

## Current subset

Supported: records, variants, enums, flags, aliases, lists, options, results, named interface functions, worlds, and standard `deps/` package references.

Refused: resources and handles, maps, tuples, chars, fixed-length lists, futures, streams, async functions, accessors, and world-defined functions/types.

Run `witforge check` against the actual package for the authoritative answer.
