---
id: M-p4
title: Protobuf emitter and glue
phase: 4
status: done
blocked_by:
- W4-001
- W4-002
- W4-003
- W4-004
- W4-005
- W4-006
- W4-007
tags:
- milestone
- p4
evidence:
  - kind: commit
    ref: 642d70cc2786453a1c4950d4e86d8d0a370703b0
---

## Exit criteria

- [x] proto messages, enums, flags, variants, wrappers, and required presence are generated
- [x] function request/response messages and method ids are generated
- [x] glue type conversions, `GlueError`, and function helpers are generated
- [x] proto and JSON round-trip tests pass
- [x] the proto command writes deterministic output and supports `--check`
