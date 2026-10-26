Flag corrections; see `domios plan --help` and `domios task --help`. `<task>` accepts id (`task_xxxxxx`), handle or key (`COD-12`); ambiguity refuses.

- `domios plan create --title "..."` — no `--goal`; the goal comes afterwards from `plan goal set`, whose `--vector` is required while the plan has no goal. A Team-bound caller without `--project` uses its Team workspace cwd.
- `domios plan task add <plan> --name "..." --sections <csv> --criterion "..."` — never `--title`, `--section` or `--acceptance`; `--criterion` repeats. Long-text flags have `-file` twins. Its `--execution-intent` is `repository_mutation|non_repository`, not the dispatch enum.
- `domios plan task update <plan> <task> --criterion "..."` — appends; add `--replace-criteria` to replace the list. `--model <provider>:<model>` is stored verbatim; a bare provider resolves at dispatch.
- `domios plan task dispatch <plan> <task> --execution-intent read_only|mutating` — intent required; no `--brief`: task body is the brief. Repeat `--authorized-gate <token>` for exact gate claims (default: none). `--kind qa|release_manager` selects a builtin prompt without changing the task's `executor_role`. `--canonical-writer` requires mutating intent and exclusive canonical ownership; refuses another live holder. Reports must match clean canonical `HEAD`.
- Choose the lightest placement: a task needing no repository is added `--execution-intent non_repository` (no checkout); review or verification dispatches `read_only` in canonical; a lone writer takes `--canonical-writer`. Only parallel `mutating` writers get lanes.
- After accepting a `--mesh` dispatch, run `domios plan task merge-remote-lane <plan> <task> <attempt>` on the origin to fetch the lane bundle and create a real merge commit.
- `domios agent spawn <provider> --role Worker --plan <plan> --execution-intent read_only --authorized-gate "<command>"` — canonical verifier: no task or `--plan-task`; reports without `--status`; runs only its gates. Mutating Workers require a task binding.
- `domios plan mode <plan> <mode>` — the mode is a positional or `--set`: `planning|approved|execution|complete`.
- `domios plan in-progress [PLAN_ID]` — the bare form scopes to the current directory's project; `--all` lists every project.

- `domios plan task files <plan> <task> --edit-file PATH --read-file PATH` — replaces both file lists (repeat flags; omit both to clear). Paths resolve against the Worker cwd at dispatch; read files must exist, edit files may be new, and no file is in both lists.

Split files and the rest of plan editing: `domios docs PLAN-EDITING`.
