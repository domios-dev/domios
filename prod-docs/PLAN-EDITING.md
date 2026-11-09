# Editing plans, specs, and tasks (token-efficient playbook)

Plan state is stored in the database. All `body` fields are markdown.
Use typed mutation commands to change plans, tasks and specs. YAML files are
explicit import/export payloads; changing a file does not update stored state.

## Read narrow, never wide

| Need | Command |
|---|---|
| One task (meta + body + context) | `domios task show <task-id>` |
| One plan section subtree | `domios plan section show <plan> <section-id-or-handle>` |
| One spec section subtree | `domios spec section show <spec> <section-id-or-handle>` |
| Section tree only (no bodies) | `domios plan get <plan> --sections` |
| Counts/coverage snapshot with bounded details | `domios plan state <plan>` |
| More state details or next actions | `domios plan state <plan> --page 2` / `domios plan next <plan> --page 2` |

State, next, and lane reads show at most 50 rows per detail list by default.
Counts, gates, liveness, and next-action decisions always cover the entire plan.
`details.totals` and `details.next_action_counts` report complete totals;
`details.windows` names shortened lists and their next page. Use `--page N`
to continue, or explicitly request `--all` when the entire result is needed.
Nested detail lists expand with `--all`; retained event and recent-stride
history windows remain bounded. Pages are ordered within each list and are recomputed from the current plan;
if the revision changes, start again at page 1. Lane cleanup uses its complete
cleanup projection regardless of display windows.

`domios plan get <plan>` renders the ENTIRE plan — goal, every section, every
task. On a large plan that is tens of thousands of tokens. Reach for it only
when you genuinely need the whole picture.

## Locate body snapshots

Read just the requested body from a real snapshot file:

```
domios plan locate <plan> --task <id-or-handle>
domios plan locate <plan> --section <id-or-handle>
domios plan locate <plan> --spec-section <id-or-handle>
# -> /abs/path/file.yaml:START-END (1-based, inclusive)
```

The response is `editable=false`. The file is a read-only snapshot of the
selected database body; editing it does not change the plan. Use `--body` or
`--body-file` with `domios plan task update`, `domios plan section update`, or
`domios spec section update` to submit changes through their typed commands.
Keep identities, timestamps, assignments and history under CLI control.

## Bulk mutations

An Orchestrator can split an approved in-progress task with
`domios plan task split <plan> <task> --child <name>` (repeat `--child`) or
`--from-file <children.json>`. Child tasks retain the parent link and inherit
sections, tags (including `work:<value>`), executor role, personality, model routing, and
quality fields unless the file overrides a field. Dispatch children individually with declared edit files; disjoint tasks share canonical, while overlap or `build-breaking` selects a lane. A shared Worker may extend its own set with `domios plan task extend-files --edit-file <path>`; conflicting live ownership refuses atomically.
Children require an explicit personality when the parent has none. Updating a task personality also renames its live local executor tab.
Draft-plan children remain locked behind ordinary plan approval.
Parents with open children cannot be marked done or cancelled until every child
is done or cancelled.
The file is a JSON array of objects; each object uses `name` and may override
`body`, `criteria`, `test_plan`, `sections`, `executor_role`, `personality`,
`role_guidance`, `model`, `model_rationale`, `correctness`, `qa_plan`, `intent`,
or `tags`. `criteria` and `tags` are arrays of strings; a child tag such as
`work:qa` replaces the inherited tag of the same facet key and plain tags are
added. `sections` accepts an array of section ids or a comma-separated string.

`domios plan record` batches upsert-task + status + steps + links into ONE
atomic write. Prefer it over N separate `task update` calls when closing out
work. (`plan.batch` RPC carries arbitrary op lists; a general CLI wrapper is
planned.)

Record from the tab that holds the task. A batch op naming a task another tab
holds is refused with `STALE TASK BINDING` and writes nothing — the plan's owner
tab is the one caller allowed to write tasks it does not execute. A shell
carrying a task id the daemon has moved should drop `--task` and let the daemon
resolve its live binding.



Task file context is structured data, separate from the brief:

```bash
domios plan task files PLAN TASK --edit-file src/new.rs --read-file docs/context.md
```

Repeat flags to attach multiple files. This replaces both lists; omit both flags to clear them. The task JSON field is `files: {edit: [...], read: [...]}`. Paths resolve against the dispatched Worker's cwd. Editable files may be new; read-only files must exist at dispatch. Core rejects overlapping resolved paths. Planners may set these structural fields through the same authorized draft mutation path as other task structure.

For a tool-less Worker, core attaches the task files and exact attempt skill snapshots through the provider's declared context-attachment capability. Unsupported attachment kinds, unreadable read-only files, or changed skill digests refuse dispatch rather than silently losing context.

Inspect completed work without reading provider history files:

```bash
domios agent turns --tab WORKER --last 10
```

This local command returns authoritative completed user/assistant turns and outcomes from the Worker's authenticated session. Agent callers must be the authenticated manager, plan owner, or target itself. User administration remains allowed. `--last` accepts 1–50; `--after` is a UTF-8 user-record lower bound. Each request scans at most 256 completed records and returns the last requested rows within that window, with `next_offset` and `has_more`. Continue with `--after NEXT_OFFSET` when `has_more` is true. Individual rows are bounded to 256 KiB and the response to 1 MiB; request fewer turns if the response budget is exceeded. Pending generations are not completed turns; plain/non-adapter sessions return an explicit unavailable reason.

`plan task add` validates all required fields together: name, executor role and role guidance. An explicit one-word personality is required, for example `--personality Ada`. A sole plan section is used when no area matches; ambiguous plans may remain unlinked with a hint. Missing `work:` tags are inferred from the area or default to `work:backend`, with a warning. Explicit invalid tags and section selectors refuse. Correctness is optional and keeps its warning. A bare provider in `--model` is resolved when dispatched.

<!-- revision: 13 -->
