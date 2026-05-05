---
title: Workspace memory — `<workspace>/.codeterm/repo.md`
area: meta
tags: [memory, workspace, skill]
---

# Workspace memory — `.codeterm/repo.md`

Each workspace can keep a small Markdown file at `<cwd>/.codeterm/repo.md` that the CodeTerm hook system auto-injects into every Claude session opened in that workspace, in a `<system-reminder>` block labelled "Workspace memory". Use it for facts that a fresh agent on this project should know without searching mem.

## What belongs here

Build / lint / test invariants, project conventions, gotchas, names of services and where they run, anything stable across sessions in this workspace.

```
- Build: `./build.sh dev` (port 7686), `./build.sh build` for release.
- Tests: `cargo test --bins` from `cli/` covers the CLI; `./build.sh test`
  for src-tauri.
- Mem store lives at `~/.codeterm/mem/knowledge.db` (prod) or
  `~/.codeterm-dev/mem/knowledge.db` (dev). Don't move it.
- Mesh peers: paired = trusted. Use `codeterm mesh peers` to list.
```

## What does NOT belong here

- Issue-specific notes ("fixing bug X" → commit message + PR description).
- Secrets (use `codeterm mem secret`).
- Anything that changes weekly (use `codeterm mem save --kind note`).
- Per-user preferences (those go in mem under `kind=preference`).

## Cross-workspace facts

If a fact applies to >1 workspace, save it to mem instead so any agent on this machine can find it:

```bash
codeterm mem save --kind fact "PgBouncer with transaction mode, max_client_conn=400"
```

## Saving discipline

Ask before saving. When the user says "remember this for the project" decide: workspace-local → `repo.md`, machine-wide → `codeterm mem save`. Don't auto-create `repo.md` files.
