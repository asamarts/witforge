---
id: M-p3
title: Rust emitter
phase: 3
status: done
blocked_by:
- W3-001
- W3-002
- W3-003
tags:
- milestone
- p3
evidence:
  - kind: commit
    ref: bd09812d28794cf8eb59a1ad4ab3a3cfc7903064
---

## Exit criteria

- [x] plain, exhaustive Rust types and interface traits are generated
- [x] per-interface submodules and version-root trait re-exports are generated
- [x] golden tests compile every mapping row
- [x] output ownership, determinism, and `--check` are implemented
