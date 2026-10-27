---
id: M-p4
title: Protobuf emitter and glue
phase: 4
status: todo
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
---

## Exit criteria

- [ ] proto messages, enums, flags, variants, wrappers, and required presence are generated
- [ ] function request/response messages and method ids are generated
- [ ] glue type conversions, `GlueError`, and function helpers are generated
- [ ] proto and JSON round-trip tests pass
- [ ] the proto command writes deterministic output and supports `--check`
