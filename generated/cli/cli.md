# Domios CLI Reference


Complete command reference. The section between the markers below is
auto-generated from the clap command tree by `domios cli-manifest --write` —
do not edit it by hand. Conventions: target tabs use `--tab`; `--json` is
a global flag on every command. For agent orientation, prefer the human-readable
workspace and tab maps, then inspect one selected record. Whole-list JSON is for
checked-in automation rather than interactive discovery.

Review status reports the configured WIP threshold and credit ledger as advisory
backpressure: `blocked` remains false, while `reason` carries a warning when the
backlog or debt reaches the threshold. Dispatch, compete, and respawn do not need
capacity grants. Prioritize review closures; all ownership, dependency, evidence,
and review-verdict requirements remain unchanged.

Tab checklists are bounded service records, independent of tab snapshots. Any agent tab, Orchestrator included, creates and keeps its own; task dispatch seeds locked items from acceptance criteria. `domios todo list` reads the current agent tab; `domios todo write --from-pad todos.json` declares the whole list as a JSON array of `{ "content", "status": "pending|in_progress|completed|blocked|skipped", "activeForm", "link?", "reason?" }`. The first write creates the checklist; later writes keep items whose content matches a title, append new content as unlocked items and remove omitted unlocked items. An existing item's link must match its current label/URL/task id; a new item's link is an http(s) URL or task reference. At most one item, omitted locked items included, is `in_progress`, and the refusal names the offending items. `blocked` and `skipped` require a `reason`. Omitted locked items stay in place and the reply reports their count and the remaining open items, as does `tab todo complete`. Writing a completed item back to `in_progress` restarts the same item: its id stays, its attempt counter increments and the reason is recorded. Every item keeps lifetime counters (started, completed, reopened, blocked, skipped) and its newest 32 events with actor, time, attempt and reason, from which attempt durations follow; older events fold into the counters. A checklist holds at most 12 items; a larger write is refused, and the remedy is a task split proposed in the report. `domios tab todo list --tab <child-id>` uses the authorized parent-list API. Parent `init` reads a JSON array of criteria strings; QA `append` reads checklist drafts and requires the current `--generation`; Orchestrator `amend` reads one draft. The Orchestrator or a QA Worker reopens one child item with `tab todo reopen <item-id> --tab <child-id> --reason "<why>"`; `plan review bounce ... --todo-item <item-id>` reopens that item with the bounce note before recording the bounce. A respawned or re-dispatched Worker on the same task inherits its predecessor's list with attempts and history. Drafts use `{ "title", "description?", "link?" }`, with link `{ "kind": "url", "url", "label" }` or `{ "kind": "domios_task", "task_ref", "plan_ref?", "label?" }`. User `reset --from-file` starts a generation from a draft array and `clear <item-id>` / `clear --all` remove unlocked items; locked items (dispatch criteria and QA checks) are never deleted, so a generation holding them refuses clear and reset and its items are marked `skipped` with a reason instead.

`workspace current` prints a bare id for shell use. Its point-query `--json`
form identifies `explicit`, `caller-tab`, `workspace-env`, or `focus-fallback`
as the source. When no identity resolves, it exits 1 with empty stdout.

**Planning?** Read `domios docs ORCHESTRATION` first (GOAL ⟶ Spec ⊥ Plan ⟶ Tasks ⟶ Strides; author GOAL-first, Spec=WHAT, Plan=HOW built from the spec, M:N section links). Don't cram requirements into plan sections; the Spec holds WHAT.

The Team verbs are `domios team list|show|rename|close|adopt-plan|adopt-agent|pair`. `domios team pair --orchestrator <provider[:model]> --planner <provider[:model]>` creates an Orchestrator + Planner Team in one action and leaves the Plan unbound until the first goal. A caller tab holding a Manager or Worker role (a Team leaf, the General Agent excepted) is refused unless it also passes `--acknowledge-leaf` — a declared, deliberate creation attributed to that calling tab, never to some other intermediary tab it might otherwise spawn to route around the refusal. `domios team close <team_id>` is the only user operation that removes a Team; it cascade-closes the Team's members, archives/detaches its plans, records owner retirement, and clears owner/planner routing to the destroyed tabs. The one automatic removal is Domios's own startup lease: the Team a `team pair` minted for its two reserved launch attempts is reaped when that launch fails, and only while it is still forming, empty, and free of any member the pair did not reserve. Every other Team - normal, active, adopted, manually created, or nonempty - survives tab and workspace closure exactly as before. Agent callers may mutate only their own `team_id` when they are the Team's Orchestrator or planner Manager; rename and close retain their authenticated direct-user path. A newly minted Team is returned with `domios team rename <team_id> <name>` and `domios docs ORCHESTRATION` guidance.

`domios team adopt-plan <team_id> <plan_id>` requires `CODETERM_TAB_ID` for the destination Team's Orchestrator or planner Manager. It activates an archived/teamless plan only when the Team has exactly one live Orchestrator and no other active plan. Adoption atomically binds the Team, replaces the old owner with that Orchestrator, records owner history, clears stale planner routing, and relinks live task hierarchy. `domios plan unarchive|resume` require an existing live Team binding; they never recreate a Team.

Agents join a Team two ways, both authorized against the caller's own membership (`CODETERM_TAB_ID` must already belong to that Team; a non-member is refused before any tab is created or mutated). `domios agent spawn <provider> --team <team_id>` attaches a fresh roleless agent at spawn time, mirroring `domios tab new --team`; it is rejected together with `--plan-task`, because a plan-bound agent already derives its Team from that binding. `domios team adopt-agent <team_id> <tab_id>` attaches an already-running roleless agent tab; adopting a tab that already belongs to a Team, holds a role, or does not exist fails with a specific error. Both bind the tab under the Team's live Orchestrator, and `domios team show <team_id>` lists the Team's member tabs.

A Team whose Orchestrator is gone is not stranded. Removing its final Orchestrator retires that owner but retains the discoverable Team and its active Plan identity/history. `domios agent spawn <provider> --team <team_id> --role Orchestrator` hands it a new one and is the only `--role` that combines with `--team`: it needs no `--workspace` (the Team names one), carries no other spawn flag, and lands on the same daemon staffing verb as the sidebar's add-member action and `POST /api/teams/{team_id}/members`. The new Orchestrator takes the Team's active plan over through the tab-gone owner rebind — owner and owner history move — so the plan heartbeat follows to the new tab; descendant role bindings follow ordinary unlink rules. If takeover fails, staffing fails and removes the spawned replacement/lane without changing the retained Team or Plan owner/history. A Team is allowed at most one **live** Orchestrator, counted as live anchoring tabs: a removed tab is gone and a dead agent's identity is cleared on exit, so neither counts, while a second live one is refused by the `team_orchestrator_already_live` error code and logged with the Team, the role, and the cause.

A Worker spawn without a task binding is permitted when it declares `--execution-intent read_only`: that is the canonical verification Worker `domios docs ORCHESTRATION-OPERATIONS` prescribes for post-merge QA. It is spawned under the plan it verifies (`--plan <plan_id>`), reports to `--parent-tab` / `--report-to`, and carries its command scope as repeated `--authorized-gate <command>` tokens, which are rendered into its brief. Holding no task, it can report against none and transition none — a report naming a task it does not hold is refused and writes nothing. The safeguard for source-changing work is unchanged: `--execution-intent mutating` without `--plan-task` (or `--plan` + `--task-id`) is refused, and so is `--role Worker --plan <id>` with neither a task binding nor read-only intent. Declare gates on the dispatch (`plan task dispatch --authorized-gate`) for a task-bound Worker; `agent spawn --authorized-gate` is rejected alongside `--plan-task`.

`domios plan task dispatch` refuses a **mutating** dispatch before any tab exists when the resolved worktree is the canonical checkout or another task's lane; the error carries a stable `violation_id` (`lane.mutating_write_in_canonical_checkout`, `lane.mutating_write_in_foreign_lane`) plus the exact `plan task update --worktree/--branch/--base` command that supplies an isolated lane. A task with no recorded lane is not refused — the dispatcher provisions one and commits Team workspace identity together with the lane. Every other lane finding is advisory: the dispatch succeeds and prints a `safety_nudges` block on stderr, each line carrying a stable id and the exact fix. Read-only dispatch, General-Agent creation, and plain shell tabs are never obstructed, including in canonical.

`domios plan task update --status pending|cancelled`, `plan task assign`, `plan task dispatch`, `plan task close`, and `plan task respawn` are available to the live Plan owner even when the former Worker's owner epoch is stale, which is the normal state once that tab is gone. A deliberate `--status blocked` update also requires a fresh nonblank `--status-label` with its reason and owner trigger, and is limited to the live Plan owner or Planner. A Worker `agent report --status blocked` is a proposal that preserves task state, assignment, lane, and report evidence until that owner decision. Stale-owner recovery retires the stale attempt in the same transition that clears the dead owner, so a late `dispatch_ready` cannot resurrect it. A non-owner caller is still refused. The live Plan owner is recognised from its own live tab binding, not from the owner record's session field, so an Orchestrator whose session moved without an `agent restart` is not locked out of its own plan. Refusals carry a stable discriminant in `data.type` — `task_owner_epoch_stale`, `task_mutation_binding_stale`, `task_owner_gone_verb_not_recoverable`, `caller_is_not_task_owner_or_plan_owner`, `task_block_authority_required` — so a tab-gone task never reports a stale epoch for a verb-eligibility problem. `domios plan archive` **succeeds** without a retrospective and emits the advisory `plan.retrospective_unrecorded` plus durable incident evidence; write the retrospective before normal closure, but it is a nudge, never a gate.

Competition has no `domios plan task` verb — it is driven by the `plan.task.compete` route, which `domios plan task show` prints for any competition-enabled task. That route gives each **mutating** competitor its own provisioned worktree/branch/base before spawn, so N competitors never share a checkout and never fall back to canonical; lanes created earlier in the call are removed if a later competitor fails. Read-only competition is unchanged.

`domios plan transfer <plan_id> --to <orchestrator-tab>` moves the plan to the destination Orchestrator's live Team. The CLI requires `CODETERM_TAB_ID`, supplies that caller identity, and the daemon requires the tab to be the source Team's Orchestrator or planner Manager; missing callers, Workers, and foreign-Team tabs cannot transfer. Transfer changes the Team, owner, owner history, and stale planner routing atomically before relinking hierarchy.

<!-- cli:auto-start -->
#### `domios`

Domios CLI — control your terminal multiplexer

- `--json` — Output raw JSON instead of human-readable text
- `--mesh` — Execute command on a remote mesh peer instead of locally

#### `domios account`

Sign in, inspect identity or sign out on this machine

#### `domios account login`

Sign in using the system browser and wait for completion

#### `domios account logout`

Sign out and delete the saved credential

#### `domios account plan`

Show the current plan and stale-cache marker

#### `domios account portal`

Open subscription management in the system browser

#### `domios account status`

Show the signed-in identity

#### `domios account upgrade`

Open an upgrade checkout in the system browser

- `<plan_key>`

#### `domios agent`

Agent hierarchy management

#### `domios agent ack`

Acknowledge receipt and ownership of the assigned task

- `--plan` — Plan id (default: $CODETERM_PLAN_ID)
- `--tab` — Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
- `--task` — Task id (default: $CODETERM_TASK_ID)

#### `domios agent allow-spawn`

Toggle whether a tab can spawn children

- `--enable` — Enable or disable spawning
- `--tab` — Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios agent child-history`

Read one recorded child's transcript under the exact parent binding

- `--binding-nonce` — Current parent binding nonce
- `--cursor` — Opaque continuation cursor from the prior response
- `--epoch` — Current parent runtime epoch
- `--root-thread-id` — Current parent thread id from its runtime binding
- `--tab` — Parent agent tab id
- `--thread` — Recorded child thread id

#### `domios agent cli`

Inspect or manage a provider's CLI using its manifest commands

#### `domios agent cli install`

Run the manifest installer in a visible utility tab and report its result

- `<provider>`

#### `domios agent cli status`

Show installed state, resolved binary, and commands for this OS

- `<provider>`

#### `domios agent cli update`

Run the manifest updater in a visible utility tab and report its result

- `<provider>`

#### `domios agent compact`

Compact agent context (sends /compact to Claude)

- `--tab` — Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios agent fork`

Open a new tab from a provider-declared session fork; bind only from its own session evidence

- `--tab`

#### `domios agent ledger`

List an orchestrator's workers from the durable ledger (survives daemon restart): branch / model / status / last-seen, one row per worker

- `--tab` — Orchestrator tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios agent link`

Link a tab as worker under a parent

- `--parent-tab` — Parent tab ID (orchestrator)
- `--tab` — Tab ID to link (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios agent login`

Sign in to the tab's runtime provider without a terminal

- `--api-key-stdin` — Read the API key from stdin; never supply credentials in argv
- `--cancel`
- `--device`
- `--tab`

#### `domios agent model`

Switch one idle agent Tab to another model. Only that Tab changes: it is relaunched on its own session and the command returns once the provider reports the new model. Providers without a live switch refuse; respawn them with `agent spawn PROVIDER --model X`

- `--force` — Explicitly override the background-work restart guard and report the affected work
- `--list-speeds` — List this tab's currently available service tiers
- `<name>` — Model id. Run `agent models PROVIDER` to list. Model id. Omit when changing only the service tier
- `--reasoning-effort` — Reasoning effort declared for the selected model
- `--runtime-mode` — Structured runtime mode declared by the selected provider
- `--speed` — Service tier id or `default` to clear the selection
- `--tab` — Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios agent models`

List available models for a specific provider

- `<provider>` — Provider id (e.g. "claude"). Required

#### `domios agent providers`

List available agent providers (claude, codex, ...) with their display names, default models, and capability flags

#### `domios agent report`

Send a report to your parent in the hierarchy

- `--acknowledge-dirty` — Acknowledge unrelated dirty files in the worktree and proceed with the readiness report. Stride is recorded acknowledging the override
- `--attach` — File to attach to the report milestone; repeat for multiple files
- `--base` — Base commit/ref for changed-file attribution (`base..HEAD` plus dirty overlay)
- `--branch` — Git branch the work landed on
- `--changed-files` — Changed files (comma-separated or repeat the flag)
- `--doc` — Markdown document to append to the report milestone
- `--from-pad` — Read the report body from a relative file in the current agent tab's pad
- `--kind` — Milestone kind when writing to --plan/--task
- `<message>` — Report message (also accepts --message alias)
- `--plan` — Plan id to append this report to (if available)
- `--sha` — Commit SHA produced
- `--skipped` — Anything deliberately skipped
- `--status` — Report outcome: in_review | blocked (proposes a task block) | partial | done
- `--tab` — Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
- `--task` — Task id to append this report to (if available)
- `--test-cmd` — Test command that was run
- `--test-result` — Result of the test command (e.g. "42 passed")
- `--text` — Report message as a flag, useful when paired with --doc

#### `domios agent restart`

Restart a live agent, or resume an exited provider's verified session in its shell

- `--force` — Skip graceful exit and force-kill; also permits --fresh on a live agent
- `--fresh` — Start a new session in the same tab. Live agents require --force; an existing exact restored session is never replaced
- `--grace-secs` — Wait this long for graceful exit before force-killing (seconds)
- `--handoff-history` — Include bounded, redacted history in a provider-changing fresh launch
- `--provider` — Relaunch with this installed PTY provider instead of the current provider
- `--tab` — Tab ID (defaults to $CODETERM_TAB_ID locally; required with --mesh)

#### `domios agent set-orchestrator`

Mark a tab as orchestrator (top-level, can spawn workers)

- `--tab` — Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios agent spawn`

Spawn a new agent; roleless unless --role is provided.

- `<agent>` — Optional agent provider id. Without one, a uniquely advertised model id selects its provider; otherwise the configured default is used
- `--args` — Extra provider args. Put this after all Domios spawn options
- `--authorized-gate` — Gate token this Worker may claim as executed; repeatable. Carried into its brief. For a task-bound Worker declare gates on the dispatch instead (`plan task dispatch --authorized-gate`)
- `--can-spawn` — Override whether an explicit Orchestrator may spawn children
- `--charter` — Watcher charter text
- `--charter-file` — Read watcher charter text from a file
- `--cwd` — Working directory: an existing directory, refused otherwise. A relative path resolves against your current directory. With --mesh it must be absolute on the peer; from Git Bash set MSYS_NO_PATHCONV=1 for POSIX paths
- `--description` — Short tab description (max 200 chars)
- `--environment` — Remote environment ID or unambiguous label; never inferred
- `--execution-intent` — Declared execution intent. Plain mutating Workers require a task binding; Worker kinds may mutate task-less and default to `mutating`
- `--interval` — Watcher tick interval, e.g. 90s, 5m, 1h
- `--kind` — Worker prompt kind/profile, with or without a task: qa, release_manager or reviewer (read-only by default)
- `--location` — Execution location declared by the provider. Remote tasks are one-shot tabs
- `--max-actions` — Max watcher actions per tick
- `--model` — Model selector (e.g. "github-copilot/claude-haiku-4.5", "gpt-5"). The provider knows whether to emit `-m X` or `--model X` — caller passes the model string opaque. Run `domios agent models <provider>` to see what's available
- `--non-user-facing` — Declare the tab non-user-facing (bench/probe): its prompts and exits never notify the user; they are reported to its parent instead
- `--nudge-cooldown` — Watcher nudge cooldown, e.g. 10m
- `--parent-tab` — Parent tab ID. Required for Workers; for a Planner, resolved from --plan (its Orchestrator) when omitted
- `--plan` — Plan id to bind — required for `--role Planner` to auto-bind Plan.planner and deliver the peer prompt naming the Orchestrator
- `--plan-task` — Plan task to bind when spawning `--role Worker` (handle, id, or prefix)
- `--preset` — Select a provider-owned preset supported by the target provider
- `--provider-reserve-pct`
- `--reason` — Why this Worker needs an unplanned task
- `--reasoning-effort` — Reasoning-effort level supported by the selected model. The provider's list is used when that model publishes no per-model levels; unsupported values are rejected, never silently downgraded
- `--report-to` — Where this worker's `agent report` should land. A tab id or `main`/`general` alias. With `--mesh PEER` it resolves on that peer, so a remote worker reports back to the spawning orchestrator. Defaults to the caller's own tab
- `--role` — Role: Worker, Manager, Orchestrator, or Planner/Watcher profile sugar
- `--sandbox` — Explicit OS confinement for this launch; Workers default to full access
- `--speed` — Provider-declared service tier for this launch
- `--system` — Override the shell system prompt of a chatBackend shell; refused for PTY agents
- `--task` — Task to send after agent starts
- `--task-file` — Read the task body from a file (or `-` for stdin) instead of --task. Local relative paths resolve from --cwd; mesh paths resolve on the caller. Content remains byte-exact. Mutually exclusive with --task
- `--task-id` — Bound task id
- `--team` — Attach this roleless agent to an existing Team, mirroring `tab new --team`. The calling tab (CODETERM_TAB_ID) must already belong to that Team. Rejected together with --plan-task, and with every --role but Orchestrator: a plan-bound agent takes its Team from that binding. `--team <id> --role Orchestrator` is the recovery add for a Team whose Orchestrator is gone and carries no other spawn flag
- `--title` — Tab title
- `--unplanned` — Create and dispatch a visible exceptional task for this Worker
- `--workspace` — Target workspace ID. Defaults to the parent tab's workspace, then your own — never the workspace selected in the UI. Pass this only to place the agent in a different workspace on purpose

#### `domios agent startup-ack`

Acknowledge that this spawned agent received its first startup turn

#### `domios agent startup-hold`

Cancel automatic cleanup for an inspected startup attempt

- `--attempt` — Launch attempt shown in the startup-overdue report
- `--tab` — Target spawned Tab

#### `domios agent switch-provider`

Continue this tab on another provider through a bounded handoff produced by the target

- `--force` — Stop the current turn before switching. Otherwise a busy tab refuses
- `--model`
- `<provider>`
- `--reasoning-effort`
- `--tab`

#### `domios agent switch-provider-status`

Read a daemon-owned provider switch operation, including self-switch results

- `<operation>`
- `--tab`

#### `domios agent turns`

Read completed turns from a tool-less agent's exact bound session

- `--after` — UTF-8 user-record lower bound; use next_offset to continue a bounded window
- `--last`
- `--tab`

#### `domios agent unlink`

Remove a tab's role. A final Orchestrator leaves its Team and Plan recoverable

- `--tab` — Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios agent usage`

Read usage or consume a confirmed reset credit for an exact tab

#### `domios agent usage read`

Read this exact tab's provider usage snapshot

- `--tab`

#### `domios agent usage reset`

Consume one available reset credit on this exact tab's account

- `--confirm` — Confirm spending one reset credit
- `--tab`

#### `domios analytics`

Manage local opt-in product analytics

#### `domios analytics disable`

Record opt-out consent and clear queued events and identifiers

#### `domios analytics enable`

Record opt-in consent and create an installation identifier

#### `domios analytics status`

Show recorded consent, queue size and delivery counters

#### `domios app`

Control the exact local native application and server lifecycle

#### `domios app quit`

Quit the exact local application after successful server cleanup and observed exits

- `--exact`

#### `domios blueprint`

Cheap hash-gated watchers with closed-set routes

#### `domios blueprint apply`

Create or update a YAML file or recipe by name in the caller's Team

- `--dry-run`
- `--like` — Offer proven graphs matching this intent before authoring
- `--note`
- `--set`
- `<source>`

#### `domios blueprint check`

Validate a YAML file or built-in recipe without a daemon

- `--set`
- `<source>`

#### `domios blueprint delete`

Delete a whole blueprint, or every stopped one in the Team; the creator or the Team owner may delete

- `<id>`
- `--stop` — Stop a running blueprint before deleting it
- `--stopped` — Delete every stopped blueprint the caller may delete in its Team

#### `domios blueprint escalation`

Resolve an open escalation

#### `domios blueprint escalation resolve`

Release a held stage and re-evaluate its observation

- `<id>`
- `<transition_id>`

#### `domios blueprint export`

Export a portable recipe containing only the definition

- `<id>`
- `--out`

#### `domios blueprint history`

Show immutable definition versions

- `<id>`

#### `domios blueprint import`

Import a portable recipe into the caller's Team (stopped by default)

- `<file>`
- `--set`
- `--start` — Start immediately after validation
- `--stopped`

#### `domios blueprint input`

Deliver a text body to a running text input stage

- `--from-pad` — Read a UTF-8 body from the caller's Agent Pad
- `<id>`
- `--text-file` — Read a UTF-8 body from a file

#### `domios blueprint keep`

Keep a generalised solution (Team Orchestrator only); give 3-5 --when intents

- `<id>`
- `--when` — One "when would someone want this" request; repeat 1-5 times, 3-5 phrased differently

#### `domios blueprint list`

List blueprints scoped to the calling tab and its Team

#### `domios blueprint log`

List bounded run journal entries, newest first

- `<id>`
- `--limit`

#### `domios blueprint new`

Print a commented YAML scaffold from a recipe or node kind

- `<kind>`
- `--like` — Offer proven graphs matching this intent before authoring

#### `domios blueprint nodes`

List trigger, step, converter and event-source capabilities

#### `domios blueprint recipe`

List built-in graph skeletons or print one ready-to-edit recipe

- `<name>`

#### `domios blueprint schema`

Print the authoring JSON Schema for editors and agents

#### `domios blueprint show`

Show one blueprint

- `<id>`

#### `domios blueprint stop`

Stop one blueprint

- `<id>`

#### `domios blueprint update`

Replace one definition at the expected version

- `--cause`
- `--dry-run` — Validate and print the resulting graph without updating it
- `--expect-version`
- `--from-file`
- `<id>`
- `--note` — Replace the Blueprint note (at most 200 plain-text characters)

#### `domios bridge`

Send a message to an external service (Telegram, etc.)

#### `domios bridge migrate`

Migrate active bridge poller from this node to another mesh peer

- `--announce` — Send a one-shot "now serving from <host>" message to the Blueprint chat
- `--announce-chat-id` — Blueprint chat id for --announce. Defaults to the target's first allow-listed bridge user
- `--claim-timeout-ms` — How long target may take to claim the Telegram lock before the migration is considered failed (ms)
- `--force` — Skip preflight target-state check (for force migrations)
- `--no-rollback` — Skip auto-rollback if target claim fails (leave source disabled)
- `--service` — Service to migrate (currently only "telegram")
- `--to` — Target peer instance_name (as shown in `domios mesh peers`)

#### `domios bridge react`

Set an emoji reaction on a message

- `<emoji>` — Emoji to react with (e.g. "👍", "✅", "❤")
- `--message-id` — Message ID to react to
- `--to` — Recipient chat/user ID

#### `domios bridge restart`

Start or restart messaging bridge (Telegram, etc.)

#### `domios bridge send`

Send a message to Telegram or other service

- `--append` — Append to previous message instead of replacing (edits existing message)
- `--buttons` — Inline keyboard buttons as JSON: [[{"text":"Yes","data":"yes"},{"text":"No","data":"no"}]]
- `--caption` — Caption for media (alternative to positional text)
- `--file` — File path for media types
- `--filename` — Override filename for document uploads
- `--from-pad` — Read the text body from the current agent tab's pad
- `--raw-text` — Preserve literal escape sequences instead of decoding \n into a newline
- `--service` — Target service
- `<text>` — Message text (also used as caption for media types)
- `--to` — Recipient chat/user ID
- `--type` — Content type: text, photo, document, voice, animation, video, sticker

#### `domios bridge thread`

Open a tab's Telegram topic thread

#### `domios bridge thread open`

Open or reuse a tab's Telegram topic and print where to find it

- `--service` — Target service
- `--tab` — Tab whose thread to open (default: $CODETERM_TAB_ID). Only the General Agent may name another tab

#### `domios canvas`

Canvas control commands

#### `domios canvas filter`

Dim the canvas to a facet value such as area:layout

- `--facet`

#### `domios canvas focus`

Focus the open canvas on a task, tab, or plan

- `--plan`
- `--tab`
- `--task`

#### `domios capabilities`

List CLI commands and clearly marked RPC-only/internal methods

#### `domios cli-manifest`

Dump the CLI command tree as JSON (source of truth for docs/prompts/cards)

- `--markdown` — Emit the Markdown command reference instead of JSON
- `--write` — Write the CLI manifest, Markdown reference and command-path exports

#### `domios config`

Read or write Domios configuration

#### `domios config get`

Get a config value (dot-notation, e.g. "integrations.telegram_enabled")

- `<key>`

#### `domios config set`

Set a config value (JSON-compatible: true, false, 42, "string")

- `<key>`
- `<value>`

#### `domios cron`

Cron job management

#### `domios cron cancel`

Stop any currently-running executions of a cron job without deleting the job definition. Future scheduled fires still run

- `<id>` — Cron job ID

#### `domios cron complete`

Mark a cron run as complete (called by agent at end of task)

- `--run-id` — Run ID
- `--summary` — Summary of what happened
- `--yes` — Confirm a human completion when this caller has no tab identity

#### `domios cron create`

Create a cron job

- `--agent` — Agent to spawn (default: claude)
- `--cwd` — Working directory
- `--description` — Description
- `--jitter-max-ms` — Max jitter in milliseconds added to each scheduled fire. 0 = off. Omit for default (10% of interval, clamped to [1m, 30m])
- `--model` — Model within the agent provider (e.g. "opus", "haiku", "gpt-5.4-mini"). Leave unset to use the provider default
- `<name>` — Short meaningful cron job name
- `--no-team` — Keep this job outside the calling tab's Team
- `--once` — Oneshot: fire once at scheduled time, then auto-delete
- `--reasoning-effort` — Reasoning-effort level advertised by the agent provider. Leave unset to use the provider default
- `--schedule` — Cron schedule (5-field: "*/30 * * * *") or oneshot time ("in 5m", ISO8601)
- `--skill` — Mem skill slug (`domios mem skill new <slug>`) to resolve and inject into the task brief at fire time as an explicit, Read-able path
- `--tab` — Tab ID to receive reports. Omit to keep runs private to the cron
- `--task` — Task prompt for the agent
- `--team` — Explicit Team membership cross-check. Jobs already inherit the calling Tab's Team
- `--timeout-secs` — Per-cron run timeout (seconds). Overrides global memory.worker_timeout_seconds. Set high (e.g. 1800) for long-running tasks like memory dreaming
- `--tz` — IANA timezone for the schedule (e.g. "Europe/Riga"). Default: UTC
- `--workspace` — Workspace ID. Defaults to the calling tab's own workspace — pass this only to schedule the job somewhere other than where you are

#### `domios cron delete`

Delete a cron job

- `<id>` — Cron job ID

#### `domios cron disable`

Disable a cron job

- `<id>` — Cron job ID

#### `domios cron edit`

Edit a cron job

- `--agent` — Agent provider override (e.g. "claude", "codex")
- `--cwd`
- `--description`
- `<id>` — Cron job ID
- `--jitter-max-ms` — Max jitter in milliseconds added to each scheduled fire. Set 0 to disable jitter. Omit to leave unchanged
- `--model` — Model within the agent provider. Pass empty string to clear
- `--name`
- `--reasoning-effort` — Reasoning-effort level advertised by the agent provider. Pass empty string to clear
- `--schedule`
- `--skill` — Mem skill slug to resolve and inject into the task brief at fire time. Pass an empty string to clear
- `--task`
- `--timeout-secs` — Per-cron run timeout (seconds). 0 clears the override (back to global default)
- `--tz` — IANA timezone (e.g. "Europe/Riga"). Pass empty string to clear back to UTC

#### `domios cron enable`

Enable a cron job

- `<id>` — Cron job ID

#### `domios cron history`

Show run history

- `<id>` — Cron job ID or name

#### `domios cron list`

List cron jobs

- `--workspace` — Filter by workspace ID

#### `domios cron trigger`

Manually trigger a cron job now

- `<id>` — Cron job ID

#### `domios curator`

Finish the current Team Curator round

#### `domios curator finish`

Validate the retro, record this round and choose the next cadence

- `--from-pad`
- `--next`

#### `domios delayed`

Manage delayed Chat UI sends

#### `domios delayed create`

Queue a delayed message

- `--at` — Absolute fire time: unix milliseconds, RFC3339, or local YYYY-MM-DDTHH:MM
- `--count` — Stop after this many delivered or skipped ticks
- `--delay` — Relative delay: 45s, 10m, 1h30m, 2d. Bare number means minutes
- `--every` — Repeat delivery at an anchored interval (minimum 1m)
- `--from-file` — Read text from a file instead of the positional arg
- `--from-pad` — Read text from a relative file in the current agent tab's pad
- `--stdin` — Read text from stdin
- `--tab` — Target tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
- `<text>` — Text to queue (positional, or omit and use --from-file/--stdin)
- `--until` — Stop recurring delivery at an absolute time

#### `domios delayed delete`

Delete a delayed message

- `<id>` — Delayed message ID

#### `domios delayed edit`

Edit a queued or failed delayed message

- `--at` — Absolute fire time: unix milliseconds, RFC3339, or local YYYY-MM-DDTHH:MM
- `--count` — Replace the recurrence tick limit
- `--delay` — Relative delay from now: 45s, 10m, 1h30m, 2d. Bare number means minutes
- `--every` — Replace the recurrence interval (minimum 1m)
- `--from-file` — Read replacement text from a file
- `--from-pad` — Read replacement text from the current agent tab's pad
- `<id>` — Delayed message ID
- `--stdin` — Read replacement text from stdin
- `--text` — Replacement text
- `--until` — Replace the recurrence end time

#### `domios delayed list`

List delayed messages for a tab

- `--tab` — Target tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios delayed send-now`

Send a delayed message immediately

- `<id>` — Delayed message ID

#### `domios docs`

Read embedded Domios documentation in full. Omit NAME to list documents

- `<name>` — Document name. Omit to list every available name and purpose

#### `domios effort`

Complete work bundles and effort lineage

#### `domios effort link`

Link two efforts by lineage

- `--from`
- `--rel` — Lineage relationship from --from to --to: follows (continues after) or supersedes (replaces)
- `--to`

#### `domios effort show`

Show the full effort bundle (linked spec(s) + plan + tasks) for one plan_id

- `<plan_id>`

#### `domios essence-map`

Generate and validate the source-backed essence map

#### `domios essence-map generate`

Generate essences/map.jsonl from the capability and core essences

- `--decisions` — Decisions JSONL path, relative to --root unless absolute
- `--map` — Map JSONL path, relative to --root unless absolute
- `--root` — Repository root containing the essences directory

#### `domios essence-map record`

Append one decision explaining the current map to the decisions log

- `--decisions` — Decisions JSONL path, relative to --root unless absolute
- `--map` — Map JSONL path, relative to --root unless absolute
- `--reason` — Why the change is correct against the source essences
- `--root` — Repository root containing the essences directory
- `--summary` — What changed in the map

#### `domios essence-map release-summary`

Summarize the map decisions of a release and offer the revert choice

- `--decisions` — Decisions JSONL path, relative to --root unless absolute
- `--map` — Map JSONL path, relative to --root unless absolute
- `--root` — Repository root containing the essences directory
- `--since` — Baseline decision id; the summary covers every decision after it

#### `domios essence-map revert`

Restore the map recorded by an earlier decision and log the revert

- `--decisions` — Decisions JSONL path, relative to --root unless absolute
- `--map` — Map JSONL path, relative to --root unless absolute
- `--root` — Repository root containing the essences directory
- `--to` — Decision id whose recorded map is restored

#### `domios essence-map validate`

Validate a generated essence map and its append-only decisions log

- `--decisions` — Decisions JSONL path, relative to --root unless absolute
- `--map` — Map JSONL path, relative to --root unless absolute
- `--root` — Repository root containing the essences directory

#### `domios identify`

Show Domios version and system info

#### `domios inbox`

Workspace Inbox — standalone tasks with no plan

#### `domios inbox add`

Add a standalone task to the workspace Inbox

- `--assign-tab`
- `--body`
- `--body-file`
- `--correctness`
- `--criterion`
- `--execution-intent`
- `--intent`
- `--intent-file`
- `--model`
- `--name`
- `--qa-plan`
- `--qa-plan-file`
- `--test-plan`
- `--type`
- `--why`
- `--why-file`

#### `domios integration`

Provider hook integration: install it with recorded consent and trust, or read its state

#### `domios integration install`

Install a provider's hook integration and trust Domios's own hooks. Running it records consent the same way the Settings install does

- `--provider` — Provider id (e.g. claude, codex)

#### `domios integration status`

Show a provider's hook integration state

- `--provider` — Provider id (e.g. claude, codex)

#### `domios integration sweep-profiles`

Preview dead provider launch artifacts; only the machine owner may remove them

- `--confirm` — Remove the profiles listed by this instance's scan
- `--include-legacy` — Include exact-pattern unmarked legacy profiles whose tabs are not live here
- `--provider` — Provider id (e.g. codex)

#### `domios keepawake`

Prevent the system from sleeping while an agent is running

#### `domios keepawake start`

Start keeping the system awake

- `--display` — Also prevent display sleep
- `--duration-secs` — Duration in seconds (omit for indefinite)
- `--gnome-policy` — On Linux/GNOME, explicitly apply current-user power policy overrides (disable inactive sleep, idle blanking, lid-close and power-button suspend)
- `--system-block` — Print system-level sudo mask commands in the status response. Domios never runs these commands silently

#### `domios keepawake status`

Show current keep-awake status (default)

#### `domios keepawake stop`

Stop the current keep-awake session

#### `domios mem`

Unified memory store — skills, facts, notes, scripts, peers, preferences

#### `domios mem append`

Append text to an existing item's body (non-destructive). Loads the current body, appends with a blank-line separator, re-indexes the embedding. Use this instead of a full `update --body` replace

- `--changed-by` — Actor label for audit trail (defaults to "user")
- `--file` — Read the text to append from a file instead (bypasses shell quoting)
- `<id>` — Mem item id
- `<text>` — Text to append (positional). Use `-` for stdin, or pass --file

#### `domios mem archive`

Archive an item without deleting it; undo with `mem restore`

- `--changed-by` — Actor label for audit trail (defaults to "user")
- `<id>`
- `--reason`

#### `domios mem area`

Merge an area reversibly; use taxonomy to list areas and counts

#### `domios mem area merge`

Move every item to another area, preserving scope and audit history

- `--dry-run`
- `<from>`
- `--into`

#### `domios mem calibration`

Memory calibration: review or trigger the dreamer's importance/archive sweep

#### `domios mem calibration log`

Show recent calibration changes (audit log)

- `--count` — Return just the count (no rows)
- `--limit` — Max rows (default 50, max 500)
- `--since` — ISO8601 lower-bound for changed_at (default: epoch)

#### `domios mem calibration stale`

Archive items untouched for a year (no access, update, relation change or touched neighbour). Gated by memory.dream_hygiene.stale_archive

- `--dry-run` — List the ids and reasons that would be archived; change nothing
- `--limit` — Dry-run list size (default: tonight's remaining cap, max 1000)

#### `domios mem calibration sweep`

Manually trigger a calibration sweep (Sunday only unless --force)

- `--force` — Bypass the day-of-week gate. Manual / testing

#### `domios mem delete`

Delete an item by id

- `<id>`

#### `domios mem dream`

Dreamer review/apply: run a daemon-routed dream phase, or apply a reviewed re-cluster plan against the local mem DB

#### `domios mem dream apply`

Re-parse a plan file and call `introduce_topic` for each YAML-fenced action block. Malformed individual fences become warnings; the rest still run. After a successful run the file is renamed to `<name>-applied.md` with a summary footer appended. Plans are hand-written or produced by a future Dreamer re-cluster phase

- `--db-path` — Override the path to knowledge.db
- `<plan_file>` — Path to the plan markdown file

#### `domios mem dream diary`

Show the most recent dream diary entries (the structured PATTERN / CONNECTION / IDEA / WHAT IF / HUBS / ONE ACTION reflections the REM agent writes after each dream cycle). Use this to read the Dreamer's fresh observations before acting on them

- `--limit` — How many entries to return (most-recent first)

#### `domios mem dream run`

Run dreaming consolidation via the daemon. Phases: light, deep, normalize, all (default). Replaces the v1.1.22 `mem dream --phase ...` form which is gone in v1.1.23

- `--phase` — Which phase to run: light | deep | normalize | all

#### `domios mem embedder`

The one embedder behind all of mem: list providers, show status, switch

#### `domios mem embedder key`

Store an embedder API key locally from stdin, with owner-only permissions

#### `domios mem embedder key set`

Atomically write <data_dir>/secrets/<provider>-embedder.key. Pipe the key on stdin; terminal input and argv keys are refused

- `<provider>` — Provider id from `mem embedder list`

#### `domios mem embedder list`

List embedder providers, their models, and whether a remote one can be activated

#### `domios mem embedder status`

Show the active provider, model, dimension, re-embed progress and vector-pending count

#### `domios mem embedder use`

Switch the embedder and re-embed every item in the background. A remote provider needs its stored key and memory.embedder.remote_opt_in

- `--dimension` — Vector dimension, for models that accept one; defaults to the model's own
- `--model` — Model id; defaults to the provider's default model
- `<provider>` — Provider id from `mem embedder list`

#### `domios mem extract-pending`

Re-run extraction workers for items that haven't been extracted yet. Useful after LLM API failures, quota resets, or bulk imports with skip_extract. Workers fire sequentially with a short delay

- `--limit` — Maximum number of items to queue (default 200)

#### `domios mem forget`

Batch-delete every item in an area (use sparingly)

- `<area>` — Area name (e.g. auth, deploy, billing)
- `--yes` — Confirm the batch delete (refused without it)

#### `domios mem get`

Print one item by id

- `<id>` — Mem item id (e.g. skill_home_assistant)

#### `domios mem graph`

Graph operations

#### `domios mem graph alias`

Merge one entity into another (alias → canonical)

- `--alias`
- `--canonical`

#### `domios mem graph augment-entity`

Patch properties on an existing entity (audit-logged)

- `--actor`
- `--id` — Entity id (e.g. person_owner)
- `--patch` — JSON object of properties to merge in (e.g. '{"city":"Riga"}')

#### `domios mem graph collapse-synonym-edges`

Collapse parallel membership-synonym edges graph-wide in one pass: per node-pair keep ONE of about/applies_to/belongs_to/part_of/documented_in, invalidate the rest. Distinct predicates (uses, runs_on, …) untouched

- `--actor`
- `--dry-run` — Print the redundant edge rowids without invalidating anything

#### `domios mem graph create`

Create a graph entity

- `--entity-type` — Entity type (Person, Project, Task, Skill, ...)
- `--id` — Entity id (e.g. person_kai)
- `--props` — JSON properties
- `--scope`

#### `domios mem graph delete-entity`

Delete an entity by id (audited). Refuses if it has active relations unless --force, in which case those relations are cascade-deleted too

- `--actor`
- `--force` — Cascade-delete active relations referencing this entity
- `--id` — Entity id (e.g. seed_person_anna)
- `--purge-history` — Also hard-delete invalidated (historical) relations referencing this entity. Default false: history is preserved as ghost rows for `revive-broken` and timeline queries. Set this when you're sure the entity is gone for good

#### `domios mem graph delete-relations`

Delete relations by explicit rowids (audited to graph_purge_log)

- `--actor`
- `--rowids` — Comma-separated rowids (e.g. "42,87,103")

#### `domios mem graph find-duplicate-entities`

Find duplicate entities (exact name match, alias chain, or vector similarity)

- `--entity-type` — Restrict `vector` strategy to entities of this type (Person, Tech, Project, ...)
- `--limit`
- `--strategy` — Strategy: exact-name | alias-chain | vector
- `--threshold` — Cosine similarity threshold for `vector` strategy (0.0-1.0). 0.90+ = strong-merge candidate, 0.85+ = review-worthy

#### `domios mem graph find-duplicate-relations`

Find duplicate relations grouped by selected columns. Read-only diagnostic — does not mutate the graph

- `--group-by` — Comma-separated columns from: from_id,relation_type,to_id,scope,valid_from. Default excludes valid_from because the relations UNIQUE(from,rel,to,valid_from) makes grouping by all four a guaranteed no-op — meaningful dedup is on the triple
- `--limit`

#### `domios mem graph get`

Inspect a graph entity: properties_json + degree summary. Filling the gap that mem search only hits mem_items, not entities. Use this when you need email/phone/role/whatever a Person/Tech/Project entity carries in its properties — `mem graph related` returns relations only

- `<id>` — Entity id (e.g. person_owner, tech_codeterm)

#### `domios mem graph hubs`

List the top hub nodes by active degree (at or above threshold). Each hub returns its degree, relation_type distribution and `clusters`: inbound neighbours grouped by shared area, title keyword or kind, each with a `suggested_topic` and member ids ready for `introduce-topic`. The dreams anchor is reported under `excluded`, not as a hub

- `--limit` — Return at most N hubs, highest degree first (default 5)
- `--sample` — Per-hub raw neighbor sample with titles (default 0: clusters only)
- `--threshold`

#### `domios mem graph introduce-topic`

Atomically introduce a topic intermediate: insert topic entity (if missing), link sources → topic → anchor, invalidate direct sources → anchor edges

- `--anchor`
- `--db-path` — Override the path to knowledge.db
- `--reason`
- `--rel-anchor`
- `--rel-source`
- `--sources` — Comma-separated source ids (s1,s2,s3) OR @path/to/file (one per line; '#'-prefixed and blank lines ignored)
- `--topic-id`
- `--type`

#### `domios mem graph invalidate`

Invalidate a relation (set valid_to, preserve history)

- `--ended` — When it ended (ISO 8601, default now)
- `--from`
- `--rel`
- `--to`

#### `domios mem graph islands`

List connected components of size <= max-size — disconnected "islands" the Dreamer should bridge or delete. Returns component member id/kind/title/area, plus a `suggested_anchor` derived from shared area / kind / title-keywords across members. JSON mode emits JSONL (one component per line, sorted by size asc)

- `--limit` — Maximum number of components to emit (caps verbose output)
- `--max-size` — Maximum component size to include (default 6 — pairs through small clusters; bigger groups are real sub-graphs)

#### `domios mem graph merge-relations`

Merge relations into `keep`, folding properties/confidence

- `--actor`
- `--keep` — Rowid of the relation to keep (its properties and rowid survive)
- `--merge` — Comma-separated rowids to fold into `keep` (these are deleted)

#### `domios mem graph orphans`

List nodes with degree 0 — true orphans (entities with no relations, mem_items not pointed at by anything). Output: id, kind, title, area, last access. JSON mode emits a flat array

- `--limit` — Cap the number of orphans emitted

#### `domios mem graph path`

Shortest path between two entities

- `--from`
- `--include-invalidated` — Include relations whose valid_to is set (default: active-only)
- `--max-hops`
- `--to`

#### `domios mem graph prune-dream-hub-edges`

Invalidate dream-diary backlinks into hubs in one pass: every edge from a daily diary to a graph entity via a noise predicate (keeps the dreams anchor)

- `--actor`
- `--dry-run` — Print the dream backlink rowids without invalidating anything

#### `domios mem graph prune-thin-vectors`

Find and (optionally) delete vectors whose mem_item has no real body (empty / title-only) — the "bodyless embeddings" that pollute search

- `--actor`
- `--dry-run` — Print the thin-body vector ids without deleting anything

#### `domios mem graph purge-invalidated`

Hard-delete relations whose valid_to is set. Optional filters by cutoff date and reason substring. Audited to graph_purge_log

- `--before` — Only purge rows with valid_to before this RFC 3339 timestamp
- `--db-path` — Override the path to knowledge.db
- `--dry-run` — Print count without modifying any rows or writing audit row
- `--reason` — Match relations.reason via LIKE %substring%

#### `domios mem graph relate`

Create a relation between entities or mem_items

- `--confidence`
- `--from`
- `--reason` — Free-text reason; written to relations.reason for audit
- `--rel`
- `--scope`
- `--to`
- `--valid-from` — Validity start (ISO 8601, default now)
- `--valid-to` — Validity end (ISO 8601, optional)

#### `domios mem graph related`

List outgoing relations from an id

- `--as-of` — As-of date (ISO 8601) — filters by valid_from/valid_to
- `--id`
- `--include-invalidated` — Include relations whose valid_to is set (default: active-only)
- `--rel` — Filter by relation type

#### `domios mem graph repair-orphan-blobs`

Find and (optionally) delete sha256_*.txt blob files in the mem blobs directory that no mem_item references. CAS-aware sweep (refcount = 0)

- `--actor`
- `--dry-run` — Print orphans without deleting anything

#### `domios mem graph repair-orphan-vectors`

Find and (optionally) delete orphan rows in mem_vectors — vectors whose mem_id has no matching mem_items / entities row. One-shot legacy cleanup

- `--actor`
- `--dry-run` — Print orphans without deleting anything

#### `domios mem graph revive-broken`

Restore valid_to=NULL for relations whose endpoints both still exist in entities or mem_items. Idempotent

- `--db-path` — Override the path to knowledge.db
- `--dry-run`
- `--since`

#### `domios mem graph route`

Atomically re-route an active direct edge (from → to) through an intermediate (from → via → to). Invalidates the original direct edge

- `--db-path` — Override the path to knowledge.db
- `--from`
- `--reason` — Free-text reason; written to relations.reason
- `--rel-in` — Relation type for the from→via segment (default = original rel)
- `--rel-out` — Relation type for the via→to segment (default = original rel)
- `--to`
- `--via`

#### `domios mem graph timeline`

Full temporal timeline for an entity

- `--entity`
- `--include-invalidated` — Include relations whose valid_to is set (default: active-only)

#### `domios mem graph traverse`

BFS traversal from an entity (max_hops cap)

- `--as-of`
- `--from`
- `--hops`
- `--include-invalidated` — Include relations whose valid_to is set (default: active-only)
- `--max-nodes`

#### `domios mem graph tunnels`

List entities bridging 2+ areas (cross-project connections)

- `--include-invalidated` — Include relations whose valid_to is set (default: active-only)

#### `domios mem history`

Print the full audit trail for one item (create/update/promote/delete)

- `<id>` — Mem item id

#### `domios mem learn`

Capture a quick learning (insight, error, feature request)

- `--error` — Shortcut: --error <text> = --kind error
- `--feature` — Shortcut: --feature <text> = --kind feature
- `--kind` — "insight" (default), "error", or "feature"
- `--tags` — Comma-separated tags
- `<text>` — What you learned. If omitted and stdin piped, read stdin

#### `domios mem learn-list`

List pending learnings (kind=learning items)

#### `domios mem learn-promote`

Promote a learning to a fact (raises importance)

- `<id>`

#### `domios mem merge`

Merge a near-duplicate into a keeper, reversibly: the lesser is archived (never deleted) and linked `merged_into` the keeper; `--conflict` links keeper `supersedes` lesser instead. Undo with `mem restore <lesser>`. Pinned, skill, rule and preference items are refused as the lesser

- `--body` — New keeper body (the merged text). Use `-` to read stdin
- `--body-file` — Read the new keeper body from a file instead of --body
- `--changed-by` — Actor label for audit trail (defaults to "user")
- `--conflict` — The two disagree: keeper supersedes lesser instead of merged_into
- `--keeper` — Item that survives
- `--lesser` — Item that is archived and linked to the keeper

#### `domios mem pin`

Pin a mem item so it always loads in L1 wake-up

- `<id>` — Mem item id (e.g. fact_my_thing)

#### `domios mem pinned`

List all currently pinned items, ordered by importance/evidence/recency

- `--limit` — Max results (default 100, max 500)

#### `domios mem purge-precompact`

Preview or purge retained raw PreCompact payloads for one already-deleted record

- `--apply` — Apply the previewed purge; default is read-only inventory
- `<id>`
- `--vacuum` — Rebuild SQLite free pages and truncate the current WAL after purge
- `--verified-deleted` — Attest prior verification of this deleted record's original flush source, summary and tags

#### `domios mem reembed`

Re-embed mem_items and/or graph entities into mem_vectors. Use this after swapping the embedder model, or to backfill existing entities that pre-date the auto-embed-on-create wiring. Daemon-routed (the daemon owns the embedder process). Idempotent

- `--scope` — What to re-embed: items (mem_items) | entities (graph) | all

#### `domios mem restore`

Undo an area merge, or restore from audit history. An archived item gets its prior kind back and its merge link invalidated (plus the --version body when given); otherwise reverts the body before the most recent change

- `--changed-by` — Actor label for audit trail (defaults to "user")
- `<id>` — Mem item id
- `--version` — History row id from `mem history <id>`: undo its area merge or restore its body. Omit to undo the latest area merge, archive, or body change

#### `domios mem run-worker`

Run a single mem maintenance pass in this subprocess. Opens the DB directly (multi-process WAL) and performs one phase of work, then exits. Used by the daemon's supervisor so a panic / OOM in dreaming or librarian cannot take the API down

- `--db-path` — Override the path to knowledge.db. Defaults to the same data dir the CLI uses elsewhere (`~/.domios/mem/knowledge.db` for prod, `~/.domios-dev/mem/knowledge.db` for dev)
- `<kind>` — Which phase to run: dreaming-light | dreaming-deep | dreaming-normalize | librarian-once

#### `domios mem save`

Save a new mem item

- `--area` — Area tag (auth | deploy | home | ...)
- `<body>` — Body content. Use `-` (or omit with piped stdin) to read stdin
- `--body-file` — Read body from a file instead of the positional arg (bypasses shell quoting)
- `--importance` — Importance 1-5 (default 3.0). Higher = more likely in L1 wake-up
- `--kind` — Item kind: skill | fact | note | conversation | script | peer | preference | daily | learning | rule | summary
- `--pin` — Pin the item for L1 wake-up (always loaded)
- `--replace` — Overwrite an existing row with the same id (upsert). Use this to update the bundled identity placeholder or any pinned record
- `--scope` — Scope: global (default) or workspace:<id>
- `--skip-extract` — Skip auto-extraction worker for this save
- `--source-path` — Path to source file on disk. Enables Librarian auto-sync
- `--summary` — Optional summary (~50-200 chars)
- `--tags` — Comma-separated tags
- `--title` — Title (will be slugified into id). If omitted, first line of body
- `--workspace` — Convenience: scope this item to workspace ID. Maps to scope="workspace:<ws>"

#### `domios mem save-preference`

Save a stable user/machine preference for wake-up context

- `--area` — Area tag (planning | orchestration | machine | ...)
- `<body>` — Preference body. Use `-` (or omit with piped stdin) to read stdin
- `--body-file` — Read body from a file instead of the positional arg
- `--importance` — Importance 1-5 (default 4.0). Higher = more likely in L1 wake-up
- `--pin` — Pin the item for L1 wake-up (always loaded)
- `--replace` — Overwrite an existing row with the same id (upsert)
- `--scope` — Scope: global (default) or workspace:<id>
- `--summary` — Optional summary (~50-200 chars)
- `--tags` — Comma-separated tags
- `--title` — Title (will be slugified into id). If omitted, first line of body
- `--workspace` — Convenience: scope this item to workspace ID. Maps to scope="workspace:<ws>"

#### `domios mem search`

Search across mem_items (FTS5 + hybrid scoring)

- `--area` — Filter by area tag
- `--kind` — Filter by kind: skill | fact | note | conversation | script | peer | preference | daily | learning | rule | summary
- `--limit` — Max results (default 5)
- `--project` — Project path whose in-project plan index should be searched too
- `<query>` — Query text
- `--rerank` — Run LLM rerank on top candidates (requires ANTHROPIC_API_KEY). Exclusive with `--supporter on`
- `--scope` — Restrict to global / workspace / workspace:<id>
- `--supporter` — Force Supporter Agent memory re-rank on or off for this search; omitted, a connected Supporter runs automatically

#### `domios mem secret`

Pluggable secret store: save / get / search / list / delete. Backend is selected in Settings (default: bitwarden once shipped; `mock` in Phase 1 builds)

#### `domios mem secret delete`

Delete by name

- `--name`
- `--yes`

#### `domios mem secret get`

Resolve and print a secret. Default = raw value to stdout (no trailing newline). Use --json for {name, value, notes}

- `--json`
- `--name`

#### `domios mem secret list`

List saved secrets (metadata only — never values)

- `--folder`
- `--limit`
- `--org`

#### `domios mem secret lock`

Lock the active backend (in-process session is dropped)

#### `domios mem secret logout`

Sign out — also drops persisted creds (OS keychain entry)

#### `domios mem secret save`

Save a secret. Value comes from --value or stdin (preferred — keeps the value out of shell history)

- `--collection`
- `--folder`
- `--name`
- `--notes`
- `--org`
- `--overwrite` — Allow replacing an existing secret with the same name
- `--value` — Plaintext value. Omit and pipe via stdin instead

#### `domios mem secret save-master-password`

Arm auto-unlock on the active backend. Refused unless the backend declares the auto-unlock capability; when it does, the password is handed to the vault, which must authenticate it before the backend persists anything in its own bucket. The CLI stores nothing itself. Reads value from stdin (preferred — out of shell history) or --password

- `--password`

#### `domios mem secret search`

Free-text search over saved secret metadata

- `<query>`

#### `domios mem secret status`

Show backend + auth status

#### `domios mem secret sync`

Pull latest from the remote vault. No-op for backends with no remote (Local OS-keychain, Mock). Bitwarden runs `bw sync`

#### `domios mem secret unlock`

Unlock the active backend with a master password (read from stdin if --password is omitted, never from argv)

- `--email`
- `--password`

#### `domios mem skill`

Skill management — folder-on-disk skills with mem index. Skills live as ~/.domios/skills/<name>/SKILL.md (+ optional support files). `add` registers the folder; Librarian re-vectorizes if SKILL.md changes

#### `domios mem skill add`

Register an existing skill folder. Reads <folder>/SKILL.md and saves a mem item with source_path linked to it. Re-runs are idempotent. Area can come from --area flag, YAML frontmatter (`area: foo`), or the parent folder name (~/.domios/skills/<area>/<skill>/SKILL.md)

- `--area` — Override the auto-detected area
- `<folder>` — Folder containing SKILL.md (e.g. ~/.domios/skills/mem-testing)
- `--importance` — Importance 1-5, default 4

#### `domios mem skill list`

List all registered skills

#### `domios mem skill new`

Scaffold a new skill folder under <skills-dir>/<name>/ (skills/ in the active instance data dir), write a SKILL.md template (with frontmatter + canonical sections) plus an empty scripts/ subdir, then register it in mem immediately. After this, edit the SKILL.md — Librarian will re-index on change

- `--area` — Optional area tag to put in the frontmatter
- `<name>` — Skill slug (also folder name). Use kebab-case

#### `domios mem stats`

Show mem store stats (counts by kind, blob sizes)

#### `domios mem sync`

Trigger file sync for source-tracked items (Librarian)

- `--path` — Sync one file by path

#### `domios mem tail`

Show recent mem writes (newest first). With --follow, poll for new rows

- `--follow` — Follow mode: poll every 500ms and print new rows as they appear
- `--limit` — Max rows per fetch (default 20)
- `--since` — ISO8601 lower-bound for the initial fetch (rows created at/after this time)

#### `domios mem taxonomy`

Print {area -> {kind -> count}} for the store (or a single workspace)

- `--workspace` — Restrict to one workspace scope

#### `domios mem unpin`

Unpin a previously pinned item

- `<id>`

#### `domios mem update`

Partial update of an existing mem item by id. Only the flags you pass are written; everything else stays as-is

- `--area`
- `--body` — New body content (re-indexes embedding). Use `-` to read stdin. REPLACES the whole body — to grow a body use `mem append`
- `--body-file` — Read the new body from a file instead of --body (bypasses shell quoting)
- `--changed-by` — Actor label for audit trail (defaults to "user"). Used by automated workflows
- `<id>` — Mem item id
- `--importance` — New importance 1-5
- `--summary`
- `--tags` — Comma-separated tags (REPLACES existing tags)
- `--title`

#### `domios mem wake-up`

Print L0+L1 wake-up context (GA init injection)

- `--budget` — Total token budget (split L0 + L1)
- `--explain` — Append a per-item L1 score breakdown for debugging selection
- `--l0-only` — Identity layer only (~100 tokens)
- `--no-l0` — Skip identity layer (return L1 only)
- `--workspace` — Workspace override; defaults to the calling tab's workspace

#### `domios mem worker`

Graph-worker operations (hidden mem panel agents)

#### `domios mem worker get`

Get one worker by id

- `<id>`

#### `domios mem worker kill`

Kill a worker by id

- `<id>`

#### `domios mem worker list`

List all graph workers

#### `domios mem worker progress`

Report progress (called by worker itself; uses $CODETERM_WORKER_ID)

- `--status`
- `<text>`

#### `domios mem worker report`

Submit final report: execution ended (uses $CODETERM_WORKER_ID)

- `--diary` — Mem id of the artifact this run produced, e.g. its dream diary id
- `--finalization` — Diary finalization outcome: succeeded | partial | failed
- `--reason` — Concrete reason for a partial or failed finalization
- `<summary>`

#### `domios mem worker spawn`

Spawn a new graph worker

- `--agent`
- `--intent`
- `--parent`
- `--task`

#### `domios mesh`

Mesh networking — peer discovery, pairing, relay

#### `domios mesh announce`

Push our current address list to paired peers without re-detecting locally. Use --to to limit to one peer

- `--to` — Optional target peer (instance_id or name). Omit for all peers

#### `domios mesh disconnect`

Disconnect (unpair) from a paired peer

- `<instance_id>` — Instance ID of the paired peer

#### `domios mesh link`

Link a machine enrolled in this account. Both machines must be signed in, online to the registry and directly reachable; no approval on the other side

- `<installation_id>` — Installation ID from the account's machine list

#### `domios mesh offline`

Go offline: stop pinging peers, stop announcing, stop refreshing local addresses (which keeps Tailscale alive). The daemon stays up; peers will time out our presence on their side. Local-only command

#### `domios mesh online`

Come back online — resume mesh activity. Local-only

#### `domios mesh peer`

Per-peer address management

#### `domios mesh peer set-addr`

Manually override our cached address for a paired peer. Useful when auto-detection is wrong or you want to point at a specific interface. Format: ip[:port]. Examples: domios mesh peer set-addr PEER-NAME 100.100.100.100 domios mesh peer set-addr PEER-NAME 192.168.1.42:7685

- `<addr>` — New address as ip[:port]
- `<peer>` — Target peer (instance_id or name)

#### `domios mesh peers`

List paired peers

#### `domios mesh refresh`

Re-detect local addresses (Tailscale + LAN) and announce them to all paired peers. Use after a network swap so peers update their cache without restarting Domios

#### `domios mesh status`

Show mesh status (identity, peers)

#### `domios mesh tabs`

List tabs on a remote mesh peer

- `<target>` — Target peer name or instance ID

#### `domios mesh unrevoke`

Explicitly lift a local peer identity revocation

- `<instance_id>` — Instance ID of the revoked peer

#### `domios model-provider`

Configure model endpoints through the installed provider capability

#### `domios model-provider add`

Add an endpoint; pipe the key on stdin. Omit --model to discover models

- `--base-url`
- `--kind`
- `--model`
- `--name`
- `--secret`

#### `domios model-provider list`

List Domios-owned endpoints without key values

#### `domios model-provider preset`

Manage Domios-owned provider presets

#### `domios model-provider preset add`

Create or replace an owned model, prompt and parameter preset

- `--model`
- `--name`
- `--prompt`
- `--steps`
- `--temperature`
- `--top-p`

#### `domios model-provider preset list`

List Domios-owned presets

#### `domios model-provider preset remove`

Remove only a Domios-owned preset

- `--name`

#### `domios model-provider remove`

Remove a Domios-owned endpoint and its owned secret

- `--name`

#### `domios model-provider test`

Check an endpoint within a bounded deadline and return its models

- `--name`

#### `domios notify`

Send, list, and clear notifications

#### `domios notify clear`

Clear all notifications

#### `domios notify list`

List notifications

- `--unread` — Show only unread notifications

#### `domios notify send`

Send a notification. The body is read only from the calling tab's AgentPad. Reports when no surface will show it

- `<body>` — REFUSED: an inline body is mangled by the shell — stage it in the pad
- `--chat-message` — The exact chat message id inside --target-tab this notification jumps to
- `--choice` — Offer one tappable choice; repeat for each. Labels only — a notification takes a chosen label back, never typed text
- `--event-key` — Stable identity for coalescing. Derived from the title when omitted
- `--from-file` — REFUSED: a file outside the pad has no provenance — stage it in the pad
- `--from-pad` — Read the notification body from a relative file in the calling tab's pad
- `--from-stdin` — REFUSED: stdin is a shell pipe — stage the body in the pad
- `--level` — What this notification is, on the one level axis. Omit to file it in the bell only
- `--offer` — Offer to turn one notification surface on: `desktop`, or a push channel's service id. The change waits for the tap — an agent may offer to be louder, never make itself louder
- `--offer-accept` — Which of this notification's own choices accepts the offer
- `--severity` — Record identity axis: info, warning, error. Tone comes from --level
- `--target-tab` — The open tab whose chat holds the message this notification is about
- `<title>` — Notification title: not blank, at most 200 characters

#### `domios orchestrator`

Orchestrator helpers

#### `domios orchestrator handoff`

Create an orchestrator handoff and optionally reseed this tab

- `--dry-run`
- `--force`
- `--from-file`
- `--goal`
- `--mode`
- `--next`
- `--plan`
- `--reseed`
- `--tab`
- `--task`

#### `domios orchestrator heartbeat`

Orchestrator heartbeat helpers

#### `domios orchestrator heartbeat show`

Show the current heartbeat and learnings

- `--plan`

#### `domios orchestrator inspect`

Show the full orchestration snapshot, including report bodies

- `--plan`
- `--tab`

#### `domios orchestrator learn`

Append a persistent learning to this orchestrator's heartbeat

- `--plan`
- `--tag`
- `<text>`

#### `domios orchestrator refresh`

Run the daemon-side refresh cycle (handoff → compact → re-orient → verify resumption)

- `--plan`
- `--tab`

#### `domios orchestrator status`

Show a compact orchestration summary

- `--plan`
- `--tab`

#### `domios pad`

Inspect the current agent tab's private ephemeral filesystem pad

#### `domios pad list`

List relative regular files in the current agent tab's pad

#### `domios pad path`

Print the current agent tab's private ephemeral pad path

#### `domios paste`

Cross-mesh ephemeral pastebin — stash payloads with short ids (#98)

#### `domios paste get`

Print a paste body to stdout. Does NOT delete unless --pop or the paste was created with --pop

- `<id>` — Paste id (e.g. "paste:a3f9k2")
- `--pop` — Delete after fetching (one-shot read)

#### `domios paste list`

List local (or --mesh <peer>) pastes — id, size, ttl_remaining

#### `domios paste put`

Stash a payload. Body from positional / --from-file / stdin

- `<body>` — Body (positional). Omit and use --from-file or --from-stdin
- `--content-type` — MIME hint (default text/plain)
- `--from-file` — Read body from a file instead
- `--from-pad` — Read body from a relative file in the current agent tab's pad
- `--from-stdin` — Read body from stdin
- `--pop` — One-shot — paste self-deletes after first fetch
- `--ttl` — TTL — e.g. "30m", "1h", "24h". Default 1h. Max 24h

#### `domios paste rm`

Explicit delete

- `<id>` — Paste id

#### `domios persona`

Inspect the shared role preset resolver — kinds, overlays, and the exact text each delivery occasion emits

#### `domios persona list`

List every role kind the resolver can reach, with refused overlays

#### `domios persona reload`

Re-read the overlay directory without restarting or rebuilding

#### `domios persona show`

Render the exact preset text a role kind receives at each delivery occasion

- `--guidance` — Task-owned persona layered over the kind's base, as a dispatch supplies it
- `--kind` — Role kind id (`orchestrator`, `planner`, a Worker subtype, ...)
- `--occasion` — Render only this occasion
- `--role` — Structural role — Manager or Worker

#### `domios plan`

Canvas plans — persistent orchestration boards

#### `domios plan adopt`

Adopt an existing plan sidecar directory into this project

- `--dry-run`
- `--overwrite`
- `--project`
- `<source>`

#### `domios plan approve`

Mark the plan approved for task generation

- `<plan_id>`

#### `domios plan archive`

Archive a plan

- `<plan_id>`
- `--yes` — Confirm archive when no Plan tab identity is available

#### `domios plan claim`

Claim an unowned plan

- `<plan_id>`

#### `domios plan control-loop`

Inspect or set a plan's control loop

#### `domios plan control-loop run`

Request one Curator round now, subject to the plan and human-wait gates

- `<plan_id>`

#### `domios plan control-loop set`

Switch the plan between Curator rounds and the plain heartbeat

- `--interval`
- `<mode>`
- `<plan_id>`

#### `domios plan control-loop show`

Show the plan's control-loop mode and heartbeat interval

- `<plan_id>`

#### `domios plan coverage`

Show section coverage (% + gaps)

- `<plan_id>`

#### `domios plan create`

Create a new plan

- `--from-spec`
- `--kinds`
- `--project`
- `--spec`
- `--spec-file`
- `--title`

#### `domios plan curator`

Grant bounded authority to the active Curator round

#### `domios plan curator grant`

Grant the Curator tab the blueprints, plan skills and saves it may use

- `--blueprint`
- `<curator_tab>`
- `--plan-skill`
- `<plan_id>`
- `--save`

#### `domios plan decision`

Decision gates — structured blocking questions routed to the planner

#### `domios plan decision answer`

Answer an open gate (immutable audit record, resumes paused tasks)

- `--answer`
- `--answered-by-tab`
- `<gate_id>`
- `<plan_id>`

#### `domios plan decision ask`

Create a decision gate, pause blocking tasks, and route the question to the planner

- `--asked-by-tab`
- `--block` — Task ids (or handles) to pause until this gate is answered
- `--context`
- `<plan_id>`
- `--question`

#### `domios plan decision escalate`

Escalate a stale open gate

- `<gate_id>`
- `--note`
- `<plan_id>`

#### `domios plan decision list`

List decision gates for a plan

- `--open` — Show only open gates
- `<plan_id>`

#### `domios plan delete`

Delete a plan. Cascades: the plan's owned spec (1:1, co-located spec.yaml) is deleted with it; specs merely linked/referenced from other plans are untouched and only lose their reference to this plan

- `<plan_id>`
- `--yes` — Confirm permanent deletion

#### `domios plan disown`

Release plan ownership

- `<plan_id>`

#### `domios plan doctor`

Inspect project-local plan files for storage and structure errors

- `--kind`
- `<plan_id>`
- `--project`

#### `domios plan element`

Generic canvas elements

#### `domios plan element add`

Add a generic canvas element of an arbitrary kind, optionally linked

- `--content`
- `--content-file`
- `--kind`
- `--link`
- `<plan_id>`

#### `domios plan element list`

List generic canvas elements

- `--kind`
- `<plan_id>`

#### `domios plan element rm`

Remove a generic canvas element

- `<element_id>`
- `<plan_id>`

#### `domios plan element show`

Show one generic canvas element

- `<element_id>`
- `<plan_id>`

#### `domios plan element update`

Update a generic canvas element's content, kind, or links

- `--content`
- `--content-file`
- `<element_id>`
- `--kind`
- `--link`
- `<plan_id>`

#### `domios plan facets`

Set declared tag facets

#### `domios plan facets set`

Set the declared tag facets (CSV)

- `--facets`
- `<plan_id>`

#### `domios plan files`

List generated section/task body files

- `<plan_id>`

#### `domios plan gaps`

Show sections with zero tasks

- `<plan_id>`

#### `domios plan get`

Render the whole plan as markdown (goal + sections + tasks); --sections for the tree only

- `<plan_id>`
- `--sections`

#### `domios plan goal`

Plan north-star goal

#### `domios plan goal set`

Update the plan's goal; fields not passed keep their current value

- `--constraint` — Replaces the constraint list; repeatable. Omit to keep, pass an empty value to clear
- `<goal_text>` — Optional trailing goal statement; stored as vision unless --vision/--vision-file is given
- `<plan_id>`
- `--postulate` — Replaces the postulate list; repeatable. Omit to keep, pass an empty value to clear
- `--vector` — Goal vector; required the first time a goal is set. Every field you omit is kept
- `--vision`
- `--vision-file`

#### `domios plan goal show`

Show the plan's north-star goal (vector, vision, constraints, postulates)

- `<plan_id>`

#### `domios plan groups`

Set task groups (phases)

#### `domios plan groups close`

Record an optional retrospective stride for a completed group

- `<key>`
- `--note`
- `<plan_id>`

#### `domios plan groups set`

Set the task groups / phases (CSV)

- `--groups`
- `<plan_id>`

#### `domios plan handoff`

Orchestrator handoffs attached to the plan

#### `domios plan handoff create`

Create a durable, goal-anchored handoff attached to the plan

- `--decision`
- `--from-file`
- `--goal`
- `--mode`
- `--next`
- `<plan_id>`
- `--rule`
- `--stdin`
- `--task`

#### `domios plan handoff list`

List the handoffs attached to a plan

- `<plan_id>`

#### `domios plan handoff show`

Show a handoff (--latest or --seq N), optionally as markdown

- `--latest`
- `--markdown`
- `<plan_id>`
- `<selector>`
- `--seq`

#### `domios plan in-progress`

List active orchestration tasks; without PLAN_ID, scope to the current directory's project

- `--all` — List active tasks across every project
- `<plan_id>` — Plan to list regardless of the caller's working directory

#### `domios plan ingest`

Ingest markdown headings as plan sections

- `<markdown>`
- `<plan_id>`

#### `domios plan kinds`

Set milestone kind columns

#### `domios plan kinds set`

Set the declared milestone kind columns (CSV)

- `--kinds`
- `<plan_id>`

#### `domios plan lane-remove`

Remove one lane worktree and its branch, refusing while a live tab's shell sits inside it

- `--branch` — Lane branch to delete after worktree removal, if merged into project HEAD
- `--force` — Force Git cleanup after cleanliness and ancestry checks; dirty content stays protected
- `--project` — Canonical checkout (needed for a missing worktree unless its task identifies the project)
- `<worktree>` — Lane worktree path to remove

#### `domios plan lanes`

Lane hygiene table: one row per open lane (task/status/tab/branch/worktree/cleanup)

- `--clean` — Preview cleanup of merged, clean, unoccupied terminal/review lanes
- `--confirm` — Execute cleanup instead of previewing it
- `--fetch` — Fetch remote refs before assessing lanes (otherwise dry run is offline)
- `<plan_id>`
- `--stale` — Only show stale lanes (dead tab / report-overdue / uncleaned)

#### `domios plan layout`

Canvas layout overrides

#### `domios plan layout set`

Set a canvas element's x/y layout override

- `--element`
- `<plan_id>`
- `--x`
- `--y`

#### `domios plan link`

Link this plan to a related plan

- `<plan_id>`
- `--to`

#### `domios plan list`

List plans

- `--active`
- `--all`
- `--archived`
- `--orchestrator`
- `--project`
- `--unowned`
- `--workspace`

#### `domios plan locate`

File + line range of a body block, for in-place edits with file tools

- `<plan_id>`
- `--section` — Plan section id or handle
- `--spec-section` — Spec section id or handle (of the plan's spec)
- `--task` — Task id or handle

#### `domios plan loop`

Self-improving loops

#### `domios plan loop create`

Declare a self-improving loop with a target and iteration budget

- `--budget`
- `<plan_id>`
- `--stop-when`
- `--target`

#### `domios plan loop record`

Record one loop iteration and leave a stride

- `<loop_id>`
- `--note`
- `--stop`
- `--task`

#### `domios plan loop set`

Update a loop's budget or stop condition

- `--budget`
- `<loop_id>`
- `--stop-when`

#### `domios plan loop status`

Show a loop's iteration count, budget, and status

- `<loop_id>`

#### `domios plan loop stop`

Stop a loop (marks it complete)

- `<loop_id>`

#### `domios plan milestone`

Task milestones

#### `domios plan milestone add`

Add a milestone (stride) to a task; --add-kind declares a new kind inline

- `--add-kind`
- `--from`
- `--kind`
- `<plan_id>`
- `<task_id>`
- `--text`
- `--text-file`

#### `domios plan milestone rm`

Remove a milestone from a task

- `<milestone_id>`
- `<plan_id>`
- `<task_id>`

#### `domios plan milestone update`

Update a milestone's kind or text

- `--kind`
- `<milestone_id>`
- `<plan_id>`
- `<task_id>`
- `--text`
- `--text-file`

#### `domios plan mode`

Set explicit effort mode: planning | approved | execution | complete

- `<mode>` — Mode value as a trailing positional (alternative to --set): planning | approved | execution | complete
- `<plan_id>`
- `--set`

#### `domios plan move`

Move a plan sidecar from another project or data dir into this project

- `--dry-run`
- `--from`
- `--overwrite`
- `<plan_id>`
- `--to`

#### `domios plan next`

Prioritized, advisory next-action guidance computed from plan state

- `<plan_id>`

#### `domios plan note`

Free-form notes on the canvas

#### `domios plan note add`

Add a free-form canvas note, optionally linked to a task or section

- `--link-section`
- `--link-task`
- `<plan_id>`
- `--text`
- `--text-file`

#### `domios plan note rm`

Remove a free-form canvas note

- `<note_id>`
- `<plan_id>`

#### `domios plan note update`

Update a free-form canvas note

- `<note_id>`
- `<plan_id>`
- `--text`
- `--text-file`

#### `domios plan pause`

Park the whole effort without changing task state or executor bindings

Aliases: park

- `<plan_id>`
- `--reason`

#### `domios plan planning-state`

Return a draft to drafting; approval uses `plan approve`

- `--note`
- `<plan_id>`
- `--set`
- `--tab` — Deprecated author label retained for draft reset records

#### `domios plan prune`

Preview or delete smoke/empty project-local plan files (dry-run by default)

- `--apply`
- `--project`

#### `domios plan qa`

Serialized QA queue for in_review tasks

#### `domios plan qa fail`

Mark QA failed, bounce to in_progress, and nudge the implementer

- `--changed-executable-surfaces` — This attempt changed executable implementation surfaces
- `--distinct-oracle` — This attempt used a materially distinct verification oracle
- `--note`
- `<plan_id>`
- `<task_id>`

#### `domios plan qa next`

Show the next queued QA task, claim it, or record a group's full-gate regression run

- `--claim`
- `--group` — Record ONE group-scoped regression working point instead of claiming a task
- `--note` — Claim note. Required with --claim
- `<plan_id>`

#### `domios plan qa override`

Authorized Orchestrator waiver of pending regression gates (owner-only, audited)

- `--gate` — Gate keys to waive; defaults to every currently blocking gate
- `<plan_id>`
- `--reason` — Why the frontier may advance past the gate — required

#### `domios plan qa pass`

Mark QA passed and close the task

- `--note`
- `<plan_id>`
- `<task_id>`

#### `domios plan qa queue`

Show the plan QA queue, or enqueue one in_review task with --task

- `--note`
- `<plan_id>`
- `--task`

#### `domios plan rebind`

Re-sync this planning binding, or restore its original Worker attempt with --task and --attempt

- `--attempt` — Exact attempt id assigned to --task
- `<plan_id>` — Optional plan id for context; required with --task and --attempt
- `--task` — Exact task whose existing Worker attempt should regain its authenticated binding

#### `domios plan reconcile`

Detect plan/task drift and optionally apply conservative fixes

- `--fix`
- `<plan_id>`

#### `domios plan record`

Record task progress with one atomic plan write

- `--body-file`
- `--key`
- `--link`
- `--name`
- `<plan_id>` — Plan id (if available from tab binding or env)
- `--sections`
- `--status` — Task status: pending | in_progress | in_review | blocked | paused | done | cancelled
- `--step`
- `--task`
- `--upsert-task`

#### `domios plan reindex`

Rebuild the project-local plan recall index

- `--project`

#### `domios plan related`

Show structural neighbors for a plan/spec/task/section unit

- `--depth`
- `--id`

#### `domios plan resume`

Resume a paused effort, restoring its previous mode

- `<plan_id>`

#### `domios plan retrospective`

Write the plan completion retrospective (plan-level stride, kind retrospective)

- `--body`
- `--body-file`
- `<plan_id>`

#### `domios plan review`

Review-backpressure: capacity, credit reconciliation, and verdicts

#### `domios plan review accept`

ACCEPT: attaches the verdict and closes the task (in_review -> done)

- `--changed-executable-surfaces`
- `--distinct-oracle`
- `--note`
- `--note-from-pad` — Read the review note from a file in this tab's pad
- `<plan_id>`
- `--residual-risk` — Past the default round bound, routes residual risk to a follow-up task
- `<task_id>`

#### `domios plan review authorize-capacity`

Grant daemon-owned review-capacity provenance to one task

- `--class`
- `<plan_id>`
- `<task_id>`

#### `domios plan review authorize-reporter`

Authorize one task-less tab to append plan-level review or canonical-QA evidence

- `--class`
- `<plan_id>`
- `<reporter_tab_id>`

#### `domios plan review bounce`

BOUNCE: atomically exits in_review back to in_progress

- `--changed-executable-surfaces`
- `--distinct-oracle`
- `--note`
- `--note-from-pad` — Read the review note from a file in this tab's pad
- `<plan_id>`
- `<task_id>`
- `--todo-item` — Reopen this Worker checklist item with the bounce note as its reason

#### `domios plan review delegate`

Hand one in-review task to a direct child QA for accept/bounce review

- `<plan_id>`
- `--qa-tab`
- `<task_id>`

#### `domios plan review hold`

HOLD: records a review hold without releasing the task assignment or lane

- `--note`
- `--note-from-pad` — Read the review note from a file in this tab's pad
- `<plan_id>`
- `<task_id>`

#### `domios plan review override`

Record a P0 exception authorizing the next over-round-limit review entry

- `--owner`
- `<plan_id>`
- `--reason`
- `--reproduction`
- `<task_id>`

#### `domios plan review reconcile-credit`

Repair historical review-credit debt from leaked review exits and audit it

- `<plan_id>`
- `--reason`

#### `domios plan review status`

Show review-backpressure status: WIP count/limit, credit, blocked reason

- `<plan_id>`

#### `domios plan review waves`

Show batch-compatible QA waves (grouped by task group)

- `<plan_id>`

#### `domios plan search`

Search synced plan/spec/task/stride history for this project

- `--active`
- `--all`
- `--archived`
- `--kind` — Restrict to plan, plan_section, spec, spec_section, task, or stride
- `--limit`
- `--project`
- `<query>`
- `--scope` — Restrict to an indexed scope such as global or workspace:<id>

#### `domios plan section`

Plan sections

#### `domios plan section add`

Add a plan section (execution phase), optionally nested under --parent

- `--body`
- `--body-file`
- `--handle`
- `--parent`
- `<plan_id>`
- `--primary`
- `--tag`
- `--title`

#### `domios plan section attach`

Attach a file to a plan section

- `<file>`
- `<plan_id>`
- `<section_id>`

#### `domios plan section rm`

Remove a plan section and its subsections

- `<plan_id>`
- `<section_id>`

#### `domios plan section show`

Show one plan section's title, body, tags, and subsections

- `<plan_id>`
- `<section_id>`

#### `domios plan section update`

Update a plan section's title, body, tags, or handle

- `--body`
- `--body-file`
- `--handle`
- `<plan_id>`
- `--primary`
- `<section_id>`
- `--tag`
- `--title`

#### `domios plan set-project`

Repair a plan's project folder after it moves, preserving plan and task identity

- `<path>`
- `<plan_id>`

#### `domios plan skill`

Plan-scoped reusable skills (folder-on-disk, ledgered, mem-indexed)

#### `domios plan skill edit`

Record a keep|discard|superseded ledger entry; --body-file replaces SKILL.md

- `--body-file`
- `--plan`
- `--rationale`
- `<slug>`
- `--verdict`

#### `domios plan skill list`

List plan skills with their latest ledger verdict

- `--plan`

#### `domios plan skill new`

Create a new plan skill folder (SKILL.md + scripts/)

- `--area`
- `--check` — Optional verification command persisted in SKILL.md frontmatter
- `--plan` — Plan id (omit for the caller Team's active plan; outside a tab, $CODETERM_PLAN_ID)
- `--rationale` — One-line ledger rationale (default: "created")
- `<slug>`
- `--title`

#### `domios plan skill show`

Print a plan skill's SKILL.md and ledger history

- `--plan`
- `<slug>`

#### `domios plan spec`

Print only the plan's hydrated section prose (no goal, tasks, or metadata)

- `<plan_id>`

#### `domios plan state`

One read-only state snapshot (counts, coverage, drift, loops, lanes, strides)

- `<plan_id>`

#### `domios plan tags`

Show tag facets, counts, topics, and duplicate warnings

- `<plan_id>`

#### `domios plan task`

Plan tasks

#### `domios plan task add`

Add a task to a plan (carries model/why routing + quality fields)

- `--assign-tab`
- `--base`
- `--body`
- `--body-file`
- `--branch`
- `--cleanup-hint`
- `--cleanup-status`
- `--correctness`
- `--correctness-file`
- `--criterion`
- `--criterion-file` — Read acceptance criteria from a file: a JSON string array, or one criterion per line. Appends to --criterion
- `--depends-on` — Task this one waits on; repeatable. Accepts a full task id (task_xxxxxx), a unique id prefix, a task handle (see --handle), or a task key (COD-12)
- `--destination`
- `--execution-intent` — Classifies whether this task changes the repository; dispatch uses read_only | mutating instead
- `--executor-role` — REQUIRED: who executes this task. Specialization scoping what surface they own, e.g. rust-backend-dev, frontend-dev, docs-sync. Presence is the contract; no vocabulary is enforced
- `--external-ref` — GitHub issue reference; repeatable. Accepts a full issue URL or owner/repository#number
- `--force`
- `--group`
- `--handle`
- `--intent`
- `--intent-file`
- `--loop-from`
- `--model`
- `--name`
- `--parent`
- `--personality` — REQUIRED: the executor's one-word persona name, e.g. Alex, Emma, Leo. Without it a dispatched worker falls back to the title 'Worker'
- `<plan_id>`
- `--primary`
- `--priority`
- `--qa-plan`
- `--qa-plan-file`
- `--role-guidance` — REQUIRED: initial worker conditioning. Presence alone is the machine contract; prose quality is a human judgment
- `--role-guidance-file` — Read required initial worker conditioning from a file
- `--sections`
- `--tag`
- `--test-plan`
- `--test-plan-file`
- `--type`
- `--why` — Routing rationale; stored as model_rationale in task JSON.
- `--why-file` — Routing rationale; stored as model_rationale in task JSON.
- `--workspace-id`
- `--workspace-name`
- `--worktree`

#### `domios plan task adopt-checkpoint`

Adopt a sealed handoff checkpoint into this authenticated Worker tab

- `<plan_id>`
- `<task_id>`

#### `domios plan task assign`

Assign a task to a tab (exports CODETERM_TASK_ID to that tab)

- `--mesh`
- `<plan_id>`
- `--tab`
- `<task_id>`

#### `domios plan task attach`

Attach a file to a plan task

- `<file>`
- `<plan_id>`
- `<task_id>`

#### `domios plan task attempt`

Show one execution attempt for a plan task

- `<attempt_id>`
- `<plan_id>`
- `<task_id>`

#### `domios plan task attempts`

List execution attempts for a plan task

- `<plan_id>`
- `<task_id>`

#### `domios plan task close`

Close a task from any lifecycle state. The authenticated Plan Orchestrator overrides worker owner-epoch drift; open subtasks still block

- `--mesh`
- `<plan_id>`
- `--result`
- `--result-file`
- `<task_id>`

#### `domios plan task dispatch`

Spawn a worker, assign the task, and deliver its brief in one atomic call

- `--agent` — Agent provider to spawn (e.g. claude, codex, opencode)
- `--authorized-gate` — Gate token the worker may claim as executed; repeatable
- `--canonical-writer` — Opt this mutating Worker into the canonical checkout as its one exclusive writer; never inferred
- `--cwd`
- `--description` — Worker tab description (defaults to a short summary of the task name)
- `--execution-intent` — Explicit task intent used by provider reserve policy
- `--kind` — Select a built-in Worker prompt kind without changing the task's executor_role
- `--model`
- `--parent-tab` — Parent tab override (defaults to the caller tab)
- `<plan_id>`
- `--provider-reserve-pct` — Optional minimum remaining provider allowance for mutating work (0 = no reserve)
- `--reasoning-effort` — Selected-model reasoning effort; provider fallback applies without per-model levels (rejected if unsupported)
- `--sandbox` — Explicit OS confinement for this launch; Workers default to full access
- `--speed` — Provider-declared service tier for this launch
- `<task_id>`
- `--workspace`

#### `domios plan task files`

Replace structured task context files (relative paths resolve against the Worker cwd)

- `--edit-file`
- `<plan_id>`
- `--read-file`
- `<task_id>`

#### `domios plan task link`

Link a task to one or more plan sections (CSV)

- `<plan_id>`
- `--sections`
- `<task_id>`

#### `domios plan task list`

List a plan's tasks, optionally filtered by status

- `--all` — Include paused, done, and cancelled tasks
- `<plan_id>`
- `--status` — Task status: pending | in_progress | in_review | blocked | paused | done | cancelled

#### `domios plan task merge-remote-lane`

Fetch a reviewed remote lane through its paired peer and merge it into the origin checkout

- `<attempt_id>`
- `<plan_id>`
- `<task_id>`

#### `domios plan task relink`

Preview or apply automatic section links for unlinked tasks (--apply)

- `--apply`
- `<plan_id>`

#### `domios plan task remote-ack`

Acknowledge a remote Worker attempt on the origin plan

- `<attempt_id>`
- `<plan_id>`
- `<task_id>`

#### `domios plan task remote-report`

Persist a remote Worker's in-review report on the origin plan

- `<attempt_id>`
- `--changed-file`
- `--message`
- `--message-file`
- `<plan_id>`
- `--sha`
- `--skipped`
- `<task_id>`
- `--test-cmd`
- `--test-result`

#### `domios plan task respawn`

Respawn a worker from one prior attempt, inheriting its recorded execution intent and provider/model

- `--attempt`
- `--model`
- `--parent`
- `<plan_id>`
- `--provider`
- `--reasoning-effort`
- `--speed` — Provider-declared service tier for this launch
- `<task_id>`
- `--title`
- `--workspace`

#### `domios plan task retag`

Replace facet/topic tags and regenerate the task handle

- `<plan_id>`
- `--primary`
- `--tag`
- `<task_id>`

#### `domios plan task rm`

Remove a task from the plan, or --all to reset generated tasks

- `--all`
- `<plan_id>`
- `<task_id>`
- `--yes`

#### `domios plan task show`

Show one task in the plan with full detail (body, criteria, test_plan, correctness, executor_role, sections)

- `--all` — With --siblings, include done/cancelled tasks
- `<plan_id>`
- `--siblings` — List sibling tasks (open-only unless --all)
- `<task_id>`

#### `domios plan task split`

Split a task into dispatchable child tasks

- `--child`
- `--from-file`
- `<plan_id>`
- `<task_id>`

#### `domios plan task update`

Update task metadata, status, quality fields, or lane assignment

- `--base`
- `--body`
- `--body-file`
- `--branch`
- `--cleanup-hint`
- `--cleanup-status`
- `--clear-dependencies`
- `--correctness`
- `--correctness-file`
- `--criterion` — Add an acceptance criterion; repeatable. Appends to the existing array by default — pass --replace-criteria to replace it wholesale instead
- `--criterion-file` — Read acceptance criteria from a file: a JSON string array, or one criterion per line. Appends to --criterion, subject to the same --replace-criteria semantics
- `--depends-on` — Add a dependency; repeatable. Appends to existing dependencies (dedup). Accepts a full task id (task_xxxxxx), a unique id prefix, a task handle, or a task key (COD-12). Use --replace-dependencies for wholesale replacement
- `--destination`
- `--executor-role` — Replace the executor role. Cannot be emptied: an update carrying a blank value is refused
- `--external-ref` — GitHub issue reference to add; repeatable. Accepts a full issue URL or owner/repository#number
- `--group`
- `--handle`
- `--intent`
- `--intent-file`
- `--model`
- `--name`
- `--parent`
- `--personality` — Executor persona name, e.g. Alex, Emma, Leo
- `<plan_id>`
- `--primary`
- `--priority`
- `--qa-plan`
- `--qa-plan-file`
- `--remove-dependency` — Remove a specific dependency edge; repeatable. Applied after --depends-on additions
- `--replace-criteria` — Replace the whole acceptance-criteria array with --criterion/--criterion-file instead of appending to it. Destructive: reports before/after counts
- `--replace-dependencies` — Replace the whole dependency set with --depends-on instead of appending
- `--role-guidance` — Replace initial worker conditioning. Cannot be emptied; presence alone is the machine contract
- `--role-guidance-file`
- `--sections`
- `--speed` — Provider-declared service tier for this launch
- `--status` — Task status: pending | in_progress | in_review | blocked | paused | done | cancelled
- `--status-label`
- `--tab`
- `--tag`
- `<task_id>`
- `--test-plan`
- `--test-plan-file`
- `--type`
- `--why` — Routing rationale; stored as model_rationale in task JSON.
- `--why-file` — Routing rationale; stored as model_rationale in task JSON.
- `--workspace-id`
- `--workspace-name`
- `--worktree`

#### `domios plan transfer`

Transfer plan ownership

- `<plan_id>`
- `--reason`
- `--to`

#### `domios plan unapprove`

Return an approved/execution plan to planning mode

- `<plan_id>`

#### `domios plan unarchive`

Restore an archived plan to active

- `<plan_id>`

#### `domios plan unlink`

Remove a plan link

- `<plan_id>`
- `--to`

#### `domios plan update`

Update plan metadata

- `--mode`
- `--note` — Waiver note (required when --planner none)
- `<plan_id>`
- `--planner` — Set attached Planner: <tab_id> to bind, or "none" to waive (requires --note)
- `--spec`
- `--spec-file`
- `--status`
- `--title`

#### `domios plugin`

Plugin management — install, update, remove, manage channels

#### `domios plugin channel`

Manage plugin channels

#### `domios plugin channel add`

Add a local path or git URL channel

- `--name`
- `<url>`

#### `domios plugin channel diff`

Compare installed plugins against a channel without installing

- `<name>`

#### `domios plugin channel ensure-default`

Register the default marketplace channel (codeterm-plugins) if missing. Register-only: nothing is installed. Best-effort/offline-tolerant

#### `domios plugin channel list`

List registered channels

#### `domios plugin channel refresh`

Refresh one channel, or all channels when name is omitted

- `<name>`

#### `domios plugin channel remove`

Remove a channel cache and registry entry

- `--force` — Allow removing the protected system channel registration
- `<name>`

#### `domios plugin config`

Configure plugin settings. Non-secret fields use the comment-preserving config.yaml; secret values are read from stdin and stored in the plugin bucket

- `--grant` — Grant a permission, persisted to the plugin's grants.yaml so the decision survives (e.g. --grant spawn_agents). Repeatable. Takes effect on the plugin's next reload
- `<id>`
- `--revoke` — Revoke a previously granted permission (repeatable)
- `--secret` — Set a declared secret field from stdin
- `--set` — Set a field: --set key=value (repeatable)
- `--values-json` — Set multiple fields from a JSON object (named --values-json so it does not collide with the global --json output flag)

#### `domios plugin get`

Show a plugin's agent-facing config and guidance: its config.yaml, settings schema, secret field presence, configHelp, agent verbs, agent guide, and installed/configured/reload state

- `<id>`

#### `domios plugin install`

Install a plugin by id from channels, or directly from a local folder

- `--channel` — Disambiguate channel installs when more than one channel offers the id
- `--from-dir` — Install from a local plugin directory
- `<id>` — Plugin id to install from registered channels
- `--id-override` — Rename the installed runtime plugin id, including plugin.json#id

#### `domios plugin list`

List installed plugins

#### `domios plugin receiver-target`

Show, set, or clear the tab that receives a webhook plugin's events

- `--clear` — Clear the binding so events return to the General Agent
- `<id>` — Plugin id that declares a webhook receiver
- `--set` — Bind this tab id as the receiver target

#### `domios plugin remove`

Remove an installed plugin

- `--force` — Allow removing a verified/system plugin folder
- `<id>`
- `--remove-data` — Also remove plugin-owned downloaded data

#### `domios plugin settings`

Manage a plugin's exact-host network grants

- `--allow-host` — Allow a host or host:port for outbound fetch (repeatable)
- `--disallow-host` — Remove a user-granted host or host:port (repeatable)
- `<id>`

#### `domios plugin update`

Update one installed plugin, or all plugins when id is omitted

- `<id>`

#### `domios plugin update-system`

Update bundled system provider plugins from the official plugin channel

- `<id>`

#### `domios plugin validate`

Validate a plugin directory without installing it: strict manifest parse (the daemon's deny-unknown-fields PluginManifest), a real QuickJS load of the entry script, then a report of which AgentProvider exports plugin.js implements and which fallbacks serve the rest. Exits non-zero on the first failure, naming it

- `<dir>` — Path to the plugin directory (the folder containing plugin.json)

#### `domios rest-api`

Inspect and recover the local REST listener through the socket

#### `domios rest-api restart`

Restart the REST listener and wait for readiness or a reported failure

#### `domios rest-api status`

Show listener phase and the last bind or runtime error over the local socket

#### `domios send`

Send text to a tab (default: current tab)

- `--actions` — Terminal actions to run after optional text, comma-separated. Keys: literal characters, enter, esc, tab, backspace, delete, up, down, left, right, home, end, pageup, pagedown, space, ctrl-c, plus delay:250ms / delay:1s pauses. Repeats: key*3 (e.g. down*3). With --actions, text is inserted first but not auto-submitted; include enter when you want to submit it
- `--at` — Queue for an absolute fire time: unix milliseconds, RFC3339, or local YYYY-MM-DDTHH:MM
- `--count` — Stop after this many delivered or skipped ticks
- `--delay` — Queue instead of sending immediately. Examples: 45s, 10m, 1h30m, 2d. Bare number means minutes. NOT the normal send path: use only for future delivery the user asked for — plain send is immediate and must never be buffered
- `--every` — Repeat delivery to this existing tab at an anchored interval (minimum 1m)
- `--from-file` — Read text from a file instead of the positional arg. Lets you pass payloads that would otherwise be eaten by shell quoting
- `--from-pad` — Read text from a relative file in the current agent tab's pad
- `--require-agent` — Refuse unless the target tab has an agent attached. Guards task dispatch so prose never lands in a raw shell (issue #8)
- `--stdin` — Read text from stdin. Pair with a single-quoted heredoc to bypass the shell entirely: domios send --tab X --stdin <<'EOF' any `backticks`, $vars, etc — zero interpolation EOF
- `--system-context` — Deliver a host system notice with this context and no reply address
- `--tab` — Target tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback). Comma-separated ids address several tabs in one call — the same payload is delivered per tab, every target is reported, and the call exits non-zero if any of them failed. Multi-target does not accept mesh ids: run it on the owning peer, one peer per call
- `<text>` — Text to send (positional, or omit and use --from-file/--stdin)
- `--to-shell` — Override the raw-shell prose guard and deliver text verbatim to a shell tab (the shell will execute it). Alias: --force
- `--until` — Stop recurring delivery at this unix millisecond, RFC3339, or local YYYY-MM-DDTHH:MM time

#### `domios service`

Manage daemon services (long-running processes monitored by Domios)

#### `domios service add`

Register a new daemon-kind service

- `--description` — Description
- `--health-degraded-values` — Comma-separated values that map to degraded (default: degraded,STARTING)
- `--health-interval` — Probe interval in seconds (default: 30)
- `--health-json-field` — JSON field to read for status (default: "status")
- `--health-mode` — Health check mode: http | tcp | process | script
- `--health-ok-values` — Comma-separated values that map to healthy (default: ok,healthy,UP)
- `--health-path` — HTTP health path (default: /healthz)
- `--host` — Host (for health_mode=tcp)
- `--kind` — Service kind (default: daemon — for cron use `domios cron create`)
- `<name>`
- `--port` — Port (for health_mode=tcp)
- `--process-label` — Process label (for health_mode=process — launchd label or systemd unit)
- `--push` — Enable push API (daemon can write to tabs via Bearer token)
- `--script-path` — Script path (for health_mode=script — exit 0 = healthy)
- `--url` — HTTP URL (for health_mode=http)

#### `domios service delete`

Delete a service (also deletes its push token + activity)

- `<id>` — Service ID

#### `domios service edit`

Edit a service

- `--enabled`
- `--health-degraded-values`
- `--health-json-field`
- `--health-ok-values`
- `--health-path`
- `<id>` — Service ID
- `--name`
- `--url`

#### `domios service list`

List all registered services (cron + daemon)

- `--enabled` — Filter by enabled state
- `--kind` — Filter by kind: cron | daemon

#### `domios service show`

Show service detail (status, config, recent activity)

- `<id>` — Service ID

#### `domios service token`

Show or rotate the push token for a service

- `<id>` — Service ID
- `--rotate` — Rotate (generate new token, invalidate old)

#### `domios sessions`

Past agent sessions — search conversations across all providers

#### `domios sessions backups`

Full-desk session backups (workspaces + tabs). List the rotated snapshots and recover a reduced/wiped desk from one without hand-editing JSON

#### `domios sessions backups export`

Export a read-only, sanitized desk snapshot with transcript size evidence

- `<name>` — Unix backup timestamp, or latest for the live desk
- `--out` — Destination JSON file. Nothing is written to the instance
- `--sanitize` — Required: message bodies, paths and secrets become same-size filler

#### `domios sessions backups import`

Replace a disposable named DEV desk with a sanitized fixture

- `--file`

#### `domios sessions backups list`

List desk snapshot backups, newest first, with workspace/tab counts

- `--limit` — Max backups shown

#### `domios sessions backups restore`

Restore a backup into the live desk. Strictly additive — it replaces NOTHING: existing workspaces and tabs are untouched, only workspaces and tabs missing from the live desk are recreated from the backup

- `<name>` — Backup name from `sessions backups list` (unix timestamp)

#### `domios sessions get`

Print metadata for a past session (provider, cwd, file path, times). Read-only probe — use before `sessions resume` to verify the session is still on disk

- `<session_id>`

#### `domios sessions resume`

Resume a past session in its original cwd. Atomic replacement for the "workspace create → agent spawn --args=--resume=UUID" dance which keeps failing because Claude/Codex look for the JSONL in the current cwd. This command: finds the session across providers, creates a workspace at the right project path, spawns an agent tab there, and hands the resume id to the provider correctly

- `--model` — Model override; translated to the provider's flag
- `<session_id>` — Session id to resume (UUID)
- `--title` — Tab title for the new tab. Defaults to "resumed <shortid>"
- `--workspace-id` — Reuse an existing workspace instead of creating a new one
- `--workspace-name` — Custom name for the new workspace (default: basename of cwd)

#### `domios sessions search`

Search past sessions by content across all registered providers. Results are tagged with `provider` so an agent can resume with the right launcher

- `--limit` — Max sessions returned
- `--max-snippets` — Max snippets shown per session
- `--project` — Restrict to sessions tagged with this project path. Handy when you only want hits from the current repo
- `--provider` — Filter by provider id (e.g. "claude"). Omit or pass "all" to search every registered provider
- `<query>` — Query string. Terms are whitespace-split; every term must appear (case-insensitive) in at least one matching message

#### `domios spec`

Requirements specs — first-class WHAT documents

#### `domios spec create`

Create a new spec

- `--project`
- `--title`

#### `domios spec delete`

Delete a spec (scrubs plan references, removes the yaml)

- `<spec_id>`
- `--yes` — Confirm permanent deletion

#### `domios spec link`

Spec-to-plan links

#### `domios spec link add`

Link spec section(s) to plan section(s) (M:N coverage edges)

- `--edge` — Repeatable coverage edge; combine many in one call
- `--plan-section`
- `<plan_id>`
- `--spec`
- `--spec-section`

#### `domios spec link list`

List spec-to-plan links for a spec

- `<spec_id>`

#### `domios spec link rm`

Remove spec-to-plan link(s); narrow with --spec-section / --plan-section

- `--plan-section`
- `<plan_id>`
- `--spec`
- `--spec-section`

#### `domios spec list`

List specs

- `--active`
- `--all`
- `--archived`
- `--project`

#### `domios spec section`

Spec sections

#### `domios spec section add`

Add a spec section (requirement), optionally nested under --parent

- `--body`
- `--body-file`
- `--handle`
- `--parent`
- `--primary`
- `<spec_id>`
- `--tag`
- `--title`

#### `domios spec section rm`

Remove a spec section and its subsections

- `<section_id>`
- `<spec_id>`

#### `domios spec section show`

Show one spec section's title, body, tags, and subsections

- `<section_id>`
- `<spec_id>`

#### `domios spec section update`

Update a spec section's title, body, tags, or handle

- `--body`
- `--body-file`
- `--handle`
- `--primary`
- `<section_id>`
- `<spec_id>`
- `--tag`
- `--title`

#### `domios spec show`

Show a hydrated spec

- `<spec_id>`

#### `domios supporter`

Configure and inspect Spinal Cord (Supporter Agent)

Aliases: spinal-cord

#### `domios supporter disconnect`

Remove a saved connection and its token; removing the active one stops the Supporter

- `<adapter>`

#### `domios supporter models`

List registered adapters and their discovered models

#### `domios supporter status`

Show connection, activity, token state and budget

#### `domios supporter token`

Manage the selected adapter's API token. The value is read from stdin only and is never printed

#### `domios supporter token clear`

Remove the stored token

- `--adapter`

#### `domios supporter token set`

Store the token piped on stdin, e.g. `pbpaste | domios supporter token set --adapter typesafe-jev`. There is deliberately no argv flag for the value

- `--adapter`

#### `domios supporter token status`

Report token presence, file path and mode (never the value). Without --adapter, every registered adapter is listed

- `--adapter`

#### `domios supporter use`

Connect or activate an adapter, remembering its own model and token

- `<adapter>`
- `--model`

#### `domios surface`

Plugin surface tabs: app tabs whose content a plugin view renders

#### `domios surface close`

Close a surface tab; refuses tabs that are not surfaces

- `--tab` — Surface tab id

#### `domios surface list`

List the surfaces enabled plugins declare and the surface tabs that are open

#### `domios surface open`

Open a plugin surface as a tab nested under its owner

- `--owner` — Tab that owns the surface (default: the calling agent tab)
- `<target>` — Surface to open, as `<plugin_id>:<surface_id>`
- `--title-from-owner` — Label the tab after its owner ("{owner}'s {title}") even when the surface is not agent-bound

#### `domios system`

OS-level integration: autostart, supervision, install hooks. Each subcommand is local-only and writes to platform-specific places (schtasks on Windows, LaunchAgents on macOS, user systemd on Linux)

#### `domios system api-token`

Print only the selected local instance API credential, without contacting the daemon

#### `domios system autostart-status`

Print whether the autostart trigger is installed and where

#### `domios system disable-autostart`

Remove the OS-level autostart trigger created by `enable-autostart`. Idempotent — running on a host without a trigger is a no-op

#### `domios system enable-autostart`

Install an OS login trigger for the external crash-aware supervisor. The supervisor alone applies clean-exit, backoff, and loop policy; platform wrappers never restart the app independently

- `--binary` — Override the path to the Domios executable. Defaults to the installed app binary for the current platform
- `--supervisor` — Select the installed Domios CLI supervisor. Defaults to the stable installed CLI; temporary worker copies are refused

#### `domios tab`

Tab management

#### `domios tab adopt-agent`

Adopt a live PTY as an agent send target without restarting it

- `--force` — Adopt even when the screen is a login shell or non-agent TUI
- `--provider` — Registered PTY provider to assert. Required; never inferred
- `--tab` — Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios tab auto-confirm`

Opt a tab in or out of auto-confirming provider permission prompts the provider positively recognizes, or show the setting with recent auto-confirmations. Questions, trust dialogs and unrecognized prompts are never answered

- `<mode>` — `on`, `off`, or `status`
- `--tab` — Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios tab capture`

Dev-only: tee a tab's raw PTY output into an asciicast v2 `.cast` file

#### `domios tab capture start`

Arm capture: tee raw PTY output (ANSI intact) + timestamps to `--out`

- `--out` — Path of the `.cast` file to write
- `--tab` — Target tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios tab capture stop`

Disarm capture: flush/close the `.cast` and write a sibling `screen.txt`

- `--tab` — Target tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios tab chat`

Read a tab's parsed chat transcript (text/thinking/tool calls/results)

- `--since` — Only return messages from this index onward
- `--tab` — Target tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios tab close`

Close a tab by ID

- `--force` — Close a Worker that still holds an in-progress task; its Orchestrator is notified with the recovery command
- `<id>` — Temporary positional alias for the tab id. Prefer --tab
- `--tab` — Tab ID to close

#### `domios tab current`

Print the current focused tab ID — no JSON parsing needed

- `--last` — Return the last tab instead of the focused one

#### `domios tab describe`

Set a short tab description (empty clears it)

- `<summary>` — Short summary, max 200 chars
- `--tab` — Tab ID to describe (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios tab exec`

Execute a command in a tab and return output

- `<command>` — Command to execute
- `--tab` — Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)
- `--timeout` — Timeout in milliseconds

#### `domios tab focus`

Focus a tab by ID

- `<id>` — Temporary positional alias for the tab id. Prefer --tab
- `--tab` — Tab ID to focus

#### `domios tab get-text`

Get visible terminal text of a tab (what the user sees)

- `--formatted` — Include raw ANSI escape codes (colors, cursor state) instead of plain text
- `--tab` — Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios tab inspect`

Show the full daemon tab payload for one or more explicit tab ids

- `--lifecycle` — Include this tab's lifecycle journal telemetry: lineage, transitions, checkpoint
- `--tab` — One or more tab ids to inspect. Repeat the flag for multiple tabs
- `<tabs>` — Temporary positional aliases for tab ids. Prefer --tab

#### `domios tab list`

List compact tab summaries across every workspace. Use `--workspace` for one workspace and `tab inspect` for full tab details

- `--agents` — Show only tabs running an agent (have an agent_type)
- `--all-workspaces` — Accepted for compatibility; every workspace is now listed by default
- `--workspace` — Workspace id to list. Without this selector every workspace is listed

#### `domios tab new`

Plain shell tab, inheriting the calling tab's workspace and Team. For an AI tab use `domios agent spawn`

- `--chat-backend` — Open a non-PTY chat-backend tab driven by the given plugin id (a plugin declaring the `chatBackend` capability). `domios send`/`tab get-text` then route to the plugin, not a shell
- `--cmd` — Spawn a shell that runs this command on start (e.g. `"npm run dev"`). Delivery waits for the PTY to be ready, so the command runs reliably even on a freshly-created tab
- `--config` — JSON config object passed to the chat-backend plugin's `openSession` (only with `--chat-backend`). E.g. `'{"model":"llama-3.1-8b"}'`
- `--cwd` — Working directory for the new tab
- `--description` — Activity summary shown below the tab title
- `--eager` — Spawn the shell PTY immediately on the daemon for headless/API use
- `--icon` — Lucide icon name (for example bug or flask-conical)
- `--list-shells` — List available shell profiles without creating a tab
- `--no-team` — Keep the new shell outside the calling tab's Team
- `--purpose` — Utility shell purpose shown in the sidebar
- `--shell` — Shell profile id (discover with --list-shells)
- `--team` — Explicit Team membership cross-check. Ordinary shells already inherit the calling Tab's Team
- `--timeout` — Agent-owned shell lifetime: positive duration (30s, 10m, 2h), or none. Requires a live creating agent (CODETERM_TAB_ID); omit for human shells
- `--title` — Tab title
- `--workspace` — Target workspace id. Defaults to the calling tab's own workspace, then the focused workspace for a caller with no tab identity

#### `domios tab rebind`

Bind a running tab to an existing provider session without restarting it

- `<session_id>` — Provider session ID to bind
- `--tab` — Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios tab release-ownership`

Release stored remote mesh control ownership from a local tab

- `--tab` — Local tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios tab rename`

Rename a tab (default: current tab)

- `--tab` — Tab ID to rename (default: $CODETERM_TAB_ID; no focused-tab fallback)
- `<title>` — New title

#### `domios tab repair`

Repair a live tab's provider identity from verified session evidence

- `--from-evidence` — Derive provider and session from this tab's live evidence. This is the default
- `--from-journal` — Restore the newest eligible journal identity backed by verified session evidence
- `--provider` — Require the bound, nonce-verified session to belong to this provider
- `--session` — Rebind this verified session and reassert its provider identity
- `--tab` — Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios tab respond`

Answer the tab's current provider-declared prompt by numbered option. Sends only when a pending prompt exists; use this instead of raw keys

- `<option>` — One-based option number shown by `tab status` or `tab inspect`
- `--tab` — Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios tab restart`

Replace a plain shell's process in the same tab, preserving tab identity and metadata

- `--cmd` — Command to execute after the replacement shell is ready
- `--cwd` — Working directory for the replacement shell
- `--tab` — Tab ID to restart (default: current tab)
- `--timeout` — Fresh shell lifetime (30s, 10m, 2h, none); defaults to the previous duration

#### `domios tab search`

Fuzzy search tabs by id-prefix, tab title, friendly session name, agent type, or current model. Cross-script — "Далтон" matches "Dalton". Add `--mesh PEER` to search a paired peer (#99). `find` kept as alias

- `<pattern>` — Substring or short pattern to match

#### `domios tab status`

Show agent status for a tab (type, provider session, prompt)

- `--tab` — Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios tab stop`

Interrupt the agent's current generation (NOT a clean exit). Sends the provider's declared stop-key sequence. Plain shells get a 200 with {stopped:false} (no key). Designed for mesh/CLI/bot drivers — type `/stop` in a Telegram bridge and have it map straight here

- `--tab` — Tab ID (default: $CODETERM_TAB_ID; no focused-tab fallback)

#### `domios tab timeout`

Reset an owned utility shell's lifetime from now, or disable it with none

- `--tab`
- `--timeout`

#### `domios tab todo`

Manage a tab-owned bounded checklist through the daemon service

#### `domios tab todo amend`

Amend one checklist item (Orchestrator only)

- `--from-file` — One JSON checklist draft from a file
- `--from-pad` — One JSON checklist draft from this tab's pad
- `<item_id>` — Stable checklist item id
- `--tab` — Child tab id

#### `domios tab todo append`

Append ordered checks to a child checklist (QA Worker only)

- `--from-file` — JSON array of checklist drafts from a file
- `--from-pad` — JSON array of checklist drafts from this tab's pad
- `--generation` — Current checklist generation, supplied by the authorized Orchestrator
- `--tab` — Child tab id

#### `domios tab todo clear`

Clear one item or the entire checklist as the user

- `--all` — Clear every item in the current generation
- `<item_id>` — Stable checklist item id; omit when using --all
- `--tab` — Target tab id (default: this tab when invoked from the host)

#### `domios tab todo complete`

Mark one checklist item complete

- `<item_id>` — Stable checklist item id
- `--tab` — Target tab id (default: this tab)

#### `domios tab todo init`

Seed a child tab's locked checklist from acceptance criteria (Orchestrator only)

- `--from-file` — JSON array of acceptance-criteria strings from a file
- `--from-pad` — JSON array of acceptance-criteria strings from this tab's pad
- `--tab` — Child tab id to initialize

#### `domios tab todo list`

List the current tab's checklist or inspect one child as its parent

- `--tab` — Child tab id. Requires this tab to be its authorized parent

#### `domios tab todo reopen`

Reopen one child checklist item with the review reason (Orchestrator or QA)

- `<item_id>` — Stable checklist item id
- `--reason` — Why the step failed review; the Worker sees it on the item
- `--tab` — Child tab id

#### `domios tab todo reset`

Reset/rewrite a checklist as the user, starting a new generation

- `--from-file` — JSON array of checklist drafts from a file
- `--tab` — Target tab id (default: this tab when invoked from the host)

#### `domios task`

Plan-backed tasks across active plans

#### `domios task drop`

Release the task assignment

- `<task_id>`

#### `domios task list`

List tasks across active plans

- `--active`
- `--all`
- `--mine`
- `--plan`
- `--status` — Task status: pending | in_progress | in_review | blocked | paused | done | cancelled
- `--unassigned`
- `--workspace`

#### `domios task show`

Show one task with plan and section context

- `--all` — With --siblings, include done/cancelled tasks
- `--siblings` — List sibling tasks (open-only unless --all)
- `<task_id>`

#### `domios task take`

Assign the task to the caller's tab

- `<task_id>`

#### `domios team`

Team registry, 1:1 plan binding, and paired creation

#### `domios team adopt-agent`

Attach an already-running roleless agent or plain shell tab to a Team (a shell becomes a utility shell, process kept)

- `<tab_id>` — Tab ID of the running agent or shell
- `<team_id>` — Team ID

#### `domios team adopt-plan`

Bind an existing plan to a Team

- `<plan_id>` — Plan ID
- `<team_id>` — Team ID

#### `domios team attach-cartographer`

Attach the one optional Cartographer Manager to a Team

- `--brief-file` — UTF-8 brief file, or - to read stdin; delivered to a new or existing Cartographer
- `--model` — Provider model ID
- `--provider` — Agent provider ID
- `<team_id>` — Team ID

#### `domios team close`

Close every member tab and remove the Team registry entry. Refused while a Team task is in progress or a Worker is working, unless --force

- `--force` — Close anyway: reports every killed Worker and task, and keeps plans with unfinished tasks active and flagged
- `<team_id>` — Team ID

#### `domios team list`

List registered Teams

#### `domios team pair`

Create an Orchestrator + Planner Team in one action. The Plan stays unbound until the first goal

- `--acknowledge-leaf` — Explicit "I know I am a leaf and I am doing this deliberately" declaration — required only when the calling tab holds a Manager or Worker role. Creation is then attributed to this tab, never to an intermediary shell
- `--orchestrator` — Orchestrator provider, optionally with a model (`provider[:model]`)
- `--planner` — Planner provider, optionally with a model (`provider[:model]`)
- `--title`
- `--workspace`

#### `domios team rename`

Give a Team a human-readable name

- `<name>` — New Team name
- `<team_id>` — Team ID

#### `domios team show`

Show one Team

- `<team_id>` — Team ID

#### `domios theme`

Theme management

#### `domios theme apply`

Apply a theme by name

- `<name>` — Theme name (builtin or custom)

#### `domios theme delete`

Delete a custom theme

- `<id>` — Theme ID to delete

#### `domios theme dir`

Show the custom themes directory path

#### `domios theme list`

List available themes

#### `domios theme save`

Save a custom theme from a TOML file

- `--apply` — Apply the theme immediately after saving
- `<file>` — Path to theme TOML file
- `<id>` — Theme ID (filename without .toml)

#### `domios todo`

Read or update the current agent tab's bounded checklist

#### `domios todo amend`

Review and amend a child's checklist; seeded criteria stay locked

- `--from-pad`
- `--note-from-pad`
- `--tab`

#### `domios todo approve`

Mark a child's shared checklist reviewed

- `--note-from-pad`
- `--tab`

#### `domios todo list`

Read this agent tab's current bounded checklist

#### `domios todo submit`

Share this tab's checklist with its parent; work may continue immediately

#### `domios todo write`

Declare this tab's whole checklist: created on first write, then reconciled; locked items stay

- `--from-pad` — JSON array of {content,status,activeForm,link?,reason?} from this tab's pad

#### `domios trace`

Read this instance's DEV flow trace without contacting the daemon

#### `domios trace recent`

List the most recently observed flows

- `--limit`

#### `domios trace show`

Show a retained flow or all flows involving a tab, with missing stages

- `<id>`
- `--since` — Limit the retained window (for example 30m, 1h, 2d)

#### `domios update`

Check for updates and manage Domios version

- `--after-update` — Message to inject into the General Agent tab after the new daemon boots. Useful for remote-triggered updates ("run the update and then verify health") — GA reads it as user input post-restart
- `--manifest-url` — Override manifest URL when paired with `install --auto`. Backend must be built with `dev-updater-allowlocal` for non-https URLs

#### `domios update check`

Check for available updates

- `--force` — Force re-check (ignore cache)

#### `domios update install`

Install update (--auto for automatic download + install + restart)

- `--after-update` — Message to inject into the General Agent tab after the new daemon boots. Lets the GA continue work post-update — e.g. "verify the new build and report version via Telegram"
- `--auto` — Use Tauri built-in updater: download, install, and restart automatically
- `--continue-prompt` — Optional prompt to send to the calling tab's own session after restart
- `--continue-tab` — Calling tab to continue. Must match `$CODETERM_TAB_ID`
- `--from-file` — Install from a local NSIS installer (Windows) or app bundle/DMG (macOS), then relaunch Domios. Bypasses the network
- `--manifest-url` — Override manifest URL (dev/test escape hatch). Requires the backend to be built with the `dev-updater-allowlocal` feature for non-https schemes; release builds reject it
- `--no-continue` — Suppress the calling tab's continuation for the next start

#### `domios usage`

Agent usage tracking — current %, projection for end-of-today / end-of-cycle based on your historical rhythm scaled to today's pace

#### `domios usage latest`

Latest usage snapshot (session + weekly %, reset times)

- `--account-id`
- `--provider`

#### `domios usage projection`

End-of-today and end-of-cycle projection based on your historical rhythm scaled by today's observed pace. Plan-independent (the rhythm is normalized; today's % anchors the absolute scale)

- `--account-id`
- `--provider`

#### `domios usage rebuild-ledger`

Re-derive KPI token usage from provider transcripts so each response counts once; safe to repeat

#### `domios usage refresh`

Re-read usage from the provider's own quota-free source (never spends quota)

- `--account-id`
- `--provider`

#### `domios web`

Web mirror helpers (share links, etc.)

#### `domios web link`

Print a ready-to-open browser UI URL for THIS machine with the API token embedded in the URL fragment (use --query for ?token= instead)

- `--host` — Host override (default: Tailscale Serve HTTPS > Tailscale IP > LAN IP > 127.0.0.1)
- `--peer` — Build a link for a paired mesh PEER (by instance name) instead of this machine: resolves the peer's host (prefers its HTTPS tailscale-serve FQDN) and mints the peer's web token. Overrides --host/--port
- `--port` — Port override (default: this daemon's API port)
- `--query` — Emit ?token= instead of #token= — easier to type, but query strings reach server/proxy logs; the fragment never does

#### `domios workspace`

Workspace management

#### `domios workspace close`

Close/delete a workspace

- `--force` — Explicitly acknowledge destroying agent tabs in this workspace
- `<id>` — Workspace ID

#### `domios workspace create`

Create a new workspace. With --cwd, reuses an existing workspace at that cwd. With --provision, the daemon creates a fresh directory for it

- `--cwd` — Working directory — agents spawned with --workspace inherit this as their CWD
- `<name>` — Workspace name
- `--provision` — Create a new empty directory under the Domios data dir as the cwd; agents started exactly there get directory trust

#### `domios workspace current`

Print this tab's workspace ID. Resolution order: --workspace, then $CODETERM_TAB_ID, then $CODETERM_WORKSPACE_ID, then the active workspace. Exits 1 with empty stdout when no identity resolves

- `--workspace` — Workspace id to report, validated against the daemon's workspace list

#### `domios workspace focus`

Focus a workspace by ID

- `<id>` — Workspace ID

#### `domios workspace list`

Show a compact human-readable workspace → Team/Plan → Tab overview

#### `domios workspace move-tab`

Move a tab to another workspace

- `<from>` — Source workspace ID
- `<tab_id>` — Tab ID to move
- `<to>` — Destination workspace ID

#### `domios workspace rename`

Rename a workspace

- `<id>` — Workspace ID
- `<name>` — New name

#### `domios workspace set-cwd`

Relocate a workspace to an existing absolute folder, preserving its identity

- `<id>`
- `<path>`


<!-- cli:auto-end -->
