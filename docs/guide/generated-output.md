---
layout: default
title: Generated output
---

# Generated output

This page shows representative output produced by the current repository build from the committed
[`examples/single`](https://github.com/asamarts/witforge/tree/main/examples/single) contract. The
repository commits these files and CI runs every generator with `--check`, so the linked files are
executable examples rather than illustrative pseudocode.

```sh
witforge rust --config examples/single/witforge.toml
witforge proto --config examples/single/witforge.toml
witforge schema --config examples/single/witforge.toml
witforge glue --config examples/single/witforge.toml
```

The source WIT defines an `item` record, an enum, flags, a variant, and one interface function.
Its [`wire.lock`](https://github.com/asamarts/witforge/blob/main/examples/single/wit/v0.1/wire.lock)
assigns the field, case, parameter, and method numbers shown below.

## Plain Rust

The Rust emitter writes owned values and one trait per WIT interface. WIT names become Rust
identifiers; no Wasm runtime types appear in this output.

```rust
pub mod types {
    #[derive(Debug, Clone, PartialEq)]
    pub struct Item {
        pub id: u64,
        pub name: ::std::string::String,
        pub tags: ::std::vec::Vec<::std::string::String>,
        pub note: ::std::option::Option<::std::string::String>,
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum Status {
        Accepted,
        Rejected,
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum Event {
        Started,
        Item(Item),
        Finished(Status),
    }
}

pub mod ingest {
    pub type Item = super::types::Item;
    pub type Event = super::types::Event;

    pub trait Ingest {
        fn submit(&self, item: Item) -> Event;
    }
}
```

Full files: [`v0_1.rs`](https://github.com/asamarts/witforge/blob/main/examples/single/generated/rust/v0_1.rs),
[`worlds.rs`](https://github.com/asamarts/witforge/blob/main/examples/single/generated/rust/worlds.rs),
and [`mod.rs`](https://github.com/asamarts/witforge/blob/main/examples/single/generated/rust/mod.rs).

## Protobuf

The protobuf emitter uses the numbers in `wire.lock`. Required scalar and string values use
proto3 `optional` so generated glue can distinguish absence from a default value. WIT interfaces
produce request and response messages plus a stable method-ID enum, not protobuf services.

```proto
message TypesItem {
  optional uint64 id = 1;
  optional string name = 2;
  repeated string tags = 3;
  optional string note = 4;
}

message TypesEvent {
  oneof witforge_value {
    Empty started = 1;
    TypesItem item = 2;
    TypesStatus finished = 3;
  }
}

message IngestSubmitRequest {
  TypesItem item = 1;
}

message IngestSubmitResponse {
  TypesEvent result = 1;
}

enum IngestMethod {
  INGEST_METHOD_UNSPECIFIED = 0;
  INGEST_METHOD_SUBMIT = 1;
}
```

Full file: [`v0.proto`](https://github.com/asamarts/witforge/blob/main/examples/single/generated/proto/v0.proto).
Generate language-specific protobuf types from this file with the normal toolchain for that
language.

## JSON Schema

The schema emitter writes one Draft 7 document per contract version. Named WIT types live under
`definitions`; records are closed objects whose properties are all required. An `option<T>` value
accepts `null`, but the containing property is still required.

```json
"types/item": {"description":"One item submitted to the ingester.","type":"object","properties":{"id":{"type":"integer","minimum":0,"maximum":18446744073709551615},"name":{"type":"string"},"tags":{"type":"array","items":{"type":"string"}},"note":{"anyOf":[{"type":"string"},{"type":"null"}]}},"required":["id","name","tags","note"],"additionalProperties":false},
"types/status": {"description":"The result state of an item.","type":"string","enum":["accepted","rejected"]},
```

Full file: [`v0_1.schema.json`](https://github.com/asamarts/witforge/blob/main/examples/single/generated/schema/v0_1.schema.json).
Read [JSON Schema and Rust Serde](json-projections.html) before treating both projections as one
JSON wire format.

## Rust/protobuf glue

The glue emitter converts borrowed plain Rust values into owned prost values and validates the
reverse conversion. Copy values are copied directly, owned values are cloned only when the target
must own them, and already-owned identity collections move out of the protobuf value without an
extra allocation.

```rust
pub fn to_proto(value: &crate::generated::rust::v0_1::types::Item) -> crate::generated::proto::v0::TypesItem {
    crate::generated::proto::v0::TypesItem {
        id: ::core::option::Option::Some(value.id),
        name: ::core::option::Option::Some(::core::clone::Clone::clone(&value.name)),
        tags: ::core::clone::Clone::clone(&value.tags),
        note: ::core::clone::Clone::clone(&value.note),
    }
}

pub fn from_proto(value: crate::generated::proto::v0::TypesItem) -> ::core::result::Result<crate::generated::rust::v0_1::types::Item, GlueError> {
    Ok(crate::generated::rust::v0_1::types::Item {
        id: match value.id { ::core::option::Option::Some(value) => value, ::core::option::Option::None => return ::core::result::Result::Err(GlueError::MissingField { field: "types/item.id" }), },
        name: match value.name { ::core::option::Option::Some(value) => value, ::core::option::Option::None => return ::core::result::Result::Err(GlueError::MissingField { field: "types/item.name" }), },
        tags: value.tags,
        note: value.note,
    })
}
```

This is an exact excerpt. Emitted Rust is deterministic but intentionally not passed through
`rustfmt`. Full files: [`v0.rs`](https://github.com/asamarts/witforge/blob/main/examples/single/generated/glue/v0.rs)
and [`mod.rs`](https://github.com/asamarts/witforge/blob/main/examples/single/generated/glue/mod.rs).

Function helpers in the same module map WIT calls to protobuf request and response messages and
expose the locked method ID:

```rust
pub const METHOD: crate::generated::proto::v0::IngestMethod = crate::generated::proto::v0::IngestMethod::Submit;

pub fn to_request(item: &crate::generated::rust::v0_1::ingest::Item) -> crate::generated::proto::v0::IngestSubmitRequest {
    crate::generated::proto::v0::IngestSubmitRequest {
        item: ::core::option::Option::Some(super::super::super::types::item::to_proto(item)),
    }
}

pub fn from_request(value: crate::generated::proto::v0::IngestSubmitRequest) -> ::core::result::Result<crate::generated::rust::v0_1::ingest::Item, GlueError> {
    ::core::result::Result::Ok(match value.item { ::core::option::Option::Some(value) => super::super::super::types::item::from_proto(value)?, ::core::option::Option::None => return ::core::result::Result::Err(GlueError::MissingField { field: "ingest#submit.item" }), })
}
```

These are conversion helpers, not a transport or RPC service implementation.

## Version adapters

With adjacent contract versions, `witforge rust` also writes `convert.rs`. The evolved example adds
an optional priority and a list of tags; old values receive their WIT-safe defaults.

```rust
/// Convert `types/task` from 0.1.0 to 0.2.0.
///
/// # Errors
///
/// Returns an error when the source value cannot be represented by the target version.
pub fn task(value: &super::__witforge_from::types::Task) -> ::core::result::Result<super::__witforge_to::types::Task, super::super::ConversionError> {
    let _ = value;
    ::core::result::Result::Ok(super::__witforge_to::types::Task {
        id: ::core::result::Result::Ok(value.id)?,
        title: ::core::result::Result::Ok(::core::clone::Clone::clone(&value.title))?,
        done: ::core::result::Result::Ok(value.done)?,
        priority: ::core::option::Option::None,
        tags: ::std::vec::Vec::new(),
    })
}
```

Full file:
[`convert.rs`](https://github.com/asamarts/witforge/blob/main/examples/evolved/generated/rust/convert.rs).
