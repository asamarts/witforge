---
layout: default
title: Generation
---

# Generation

## Commands

```sh
witforge rust wit --out generated/rust
witforge proto wit --out generated/proto
witforge schema wit --out generated/schema
witforge glue wit --out generated/glue \
  --plain-root crate::plain \
  --proto-root crate::proto
```

`--plain-root` and `--proto-root` are Rust module paths used by generated glue. They are not filesystem paths.

## Configuration

```toml
version = 1

[project.example]
root = "wit"

[project.example.rust]
out = "generated/rust"
serde = true

[project.example.proto]
out = "generated/proto"

[project.example.schema]
out = "generated/schema"

[project.example.glue]
out = "generated/glue"
plain-root = "crate::plain"
proto-root = "crate::proto"
```

Then run:

```sh
witforge rust --config witforge.toml
witforge proto --config witforge.toml
witforge schema --config witforge.toml
witforge glue --config witforge.toml
```

## Output ownership

Each emitter needs its own output directory. witforge only overwrites or removes files with that emitter's generated header. It refuses symlinks, directories, and unowned files at expected output paths.

Every generator supports `--check`, which writes nothing and fails on missing, changed, or extra generated files.

## Inspect the result

The [generated-output guide](generated-output.html) shows current Rust, protobuf, JSON Schema,
glue, and version-adapter output from the committed runnable examples. The full files are checked
with `--check` in this repository's CI.
