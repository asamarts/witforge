---
id: M-p0
title: Adopt aplan for witforge tasking and dogfooding
phase: 0
status: done
blocked_by:
- W0-001
tags:
- milestone
- p0
evidence:
  - kind: commit
    ref: 2eaf22a
---

## Exit criteria

- [x] `.aplan.yml`, the plan tasks, `aplan/README.md`, and the pilot log exist
- [x] `AGENTS.md` carries the generated aplan block and project instructions
- [x] `CLAUDE.md` is a symlink to `AGENTS.md`
- [x] `aplan check`, `aplan verify`, and `aplan export-agents-md --check` pass
