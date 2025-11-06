---
sidebar_position: 2
---

# Specs Workflow

BranchBox keeps feature specifications under version control and moves them between lifecycle folders as worktrees advance.

## Directory Layout

```
docs/features/
├── backlog/       # Future work; created manually or by product
├── in-progress/   # Active features while a worktree exists
└── completed/     # Automatically promoted during teardown with --complete-spec
```

Each file is Markdown with YAML front matter. The Specs module (`core/src/modules/specs.rs`) updates metadata and creates scaffolding when a worktree starts.

## Automation Touchpoints

- `branchbox feature start`: promotes backlog entry or creates a stub in `in-progress/`.
- `branchbox feature teardown --complete-spec`: moves the spec into `completed/`.
- Specs respect the `FEATURES_DIR` environment variable if a project stores specifications elsewhere.

Refer to `AGENTS.md` for expectations around keeping specs and documentation synchronized.
