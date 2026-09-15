# Editing plans, specs, and tasks (token-efficient playbook)

Plan state is per-entity yaml in the project: `<plan>/plan.yaml` (sections
inline), `<plan>/goal.yaml`, `<plan>/spec.yaml` (the plan's spec, 1:1),
`<plan>/tasks/<handle>.yaml`, plus append-only stride logs
`<plan>/tasks/<handle>/strides.jsonl`. **All `body` fields are markdown.**

## Read narrow, never wide

| Need | Command |
|---|---|
| One task (meta + body + context) | `codeterm task show <task-id>` |
| One plan section subtree | `codeterm plan section show <plan> <section-id-or-handle>` |
| One spec section subtree | `codeterm spec section show <spec> <section-id-or-handle>` |
| Section tree only (no bodies) | `codeterm plan get <plan> --sections` |
| Counts/coverage snapshot | `codeterm plan state <plan>` |

`codeterm plan get <plan>` renders the ENTIRE plan — goal, every section, every
task. On a large plan that is tens of thousands of tokens. Reach for it only
when you genuinely need the whole picture.

## Edit bodies in place with your file tools

Bodies are yaml block scalars — you may edit them directly with your standard
Read/Edit tools; the daemon's watcher reconciles the change. Locate the exact
lines first:

```
codeterm plan locate <plan> --task <id-or-handle>
codeterm plan locate <plan> --section <id-or-handle>
codeterm plan locate <plan> --spec-section <id-or-handle>
# -> /abs/path/file.yaml:START-END  (1-based, inclusive)
```

Then `Read` that range and `Edit` within it. Rules:

- **Keep the block indentation** — every body line keeps the same leading
  spaces as the existing lines; the yaml breaks otherwise.
- Write **markdown** in the body (it renders as markdown in the UI).
- Empty/inline bodies (`body: ''`, `body: null`) are not block-editable —
  `locate` says so; set them via CLI (`--body` / `--body-file`) instead.
- Growing or shrinking the line count is fine; only indentation is sacred.

Direct edits suit iterating on prose (fixing a paragraph inside a large body).
For replacing a whole body, CLI `--body-file` is equally cheap.

## Never edit by hand

- `id`, `handle`, timestamps — identity and ordering; CLI owns them.
- `status`, assignments, lane fields — go through
  `codeterm plan task update` / `codeterm plan record` so memory, index, and
  events stay in sync.
- `strides.jsonl` — append-only via `codeterm plan milestone add` /
  `codeterm plan record --step`; never rewrite.

## Bulk mutations

`codeterm plan record` batches upsert-task + status + steps + links into ONE
atomic write. Prefer it over N separate `task update` calls when closing out
work. (`plan.batch` RPC carries arbitrary op lists; a general CLI wrapper is
planned.)

Record from the tab that holds the task. A batch op naming a task another tab
holds is refused with `STALE TASK BINDING` and writes nothing — the plan's owner
tab is the one caller allowed to write tasks it does not execute. A shell
carrying a task id the daemon has moved should drop `--task` and let the daemon
resolve its live binding.

<!-- revision: 3 -->
