# Tab lifecycle audit and exact-checkpoint recovery

CodeTerm keeps one append-only journal of every authoritative tab lifecycle
transition. A restart may hand a tab back its provider, session, role, and Team
authority **only** from a complete committed checkpoint whose lineage is exactly
the snapshot's. Anything unproven fails closed: the tab comes back as a plain
shell and the reason is logged.

This exists because a tab can reach shell readiness during a spawn and never
commit provider/session/team identity. The session snapshot still holds the
role residue, and on restart that residue used to come back as a role-bearing
plain shell with no provider session behind it.

## Where it lives

| Instance | Path |
|----------|------|
| Production | `~/.codeterm/sessions/tab-lifecycle.jsonl` |
| Dev | `~/.codeterm-dev/sessions/tab-lifecycle.jsonl` |
| Named dev instance | `~/.codeterm-dev-<name>/sessions/tab-lifecycle.jsonl` |

It sits beside `last.json`, `backups/`, and `agent-session-identities.json` in
the same `sessions/` directory.

**Agent access is read-only.** Read it, grep it, diff it. Never hand-edit the
journal or a session snapshot: an edited journal is indistinguishable from a
forged commit, and the recovery rule exists precisely to refuse claims it cannot
prove.

## Format

One JSON object per line, in journal order. Two record kinds:

```jsonc
// a committed transition
{"record":"transition","entry":{
  "seq":41,
  "key":{"tab_id":"db79d2da","epoch":3,"operation_id":"6f1c…"},
  "transition":{"transition":"session_bound","session_id":"…","binding_nonce":"…"},
  "recorded_at":"2026-08-04T11:20:31Z"
}}

// a compacted operation
{"record":"checkpoint","checkpoint":{
  "key":{"tab_id":"db79d2da","epoch":3,"operation_id":"6f1c…"},
  "committed_through":46,
  "authority":{"provider":"claude","session_id":"…","binding_nonce":"…",
               "role":"Manager","role_profile":"orchestrator",
               "team_id":"team_…","can_spawn":true,"depth":1},
  "sealed_at":"2026-08-04T11:20:33Z"
}}
```

### Lineage

`key` is the whole identity of one lifecycle operation:

- `tab_id` — the tab.
- `epoch` — monotonic per tab; a new one opens at every launch.
- `operation_id` — a UUID minted with the epoch.

Two of the three matching is not a match.

### Ordering

`seq` is a single monotonic sequence across the whole file. It is the **only**
ordering authority. `recorded_at` and `sealed_at` are UTC stamps for reading the
journal by eye — they are never consulted for ordering, lineage, checkpoint
selection, or conflict resolution.

A line that fails to parse, or whose `seq` does not advance, refuses itself and
everything after it. A truncated tail never silently becomes history, and while
the tail is refused every authority claim fails closed.

### Transitions

| `transition` | Meaning |
|--------------|---------|
| `epoch_opened` | A launch operation begins. No authority yet. |
| `provider_identity_committed` | Provider accepted for this operation. |
| `session_bound` | Provider session bound and proven by the echoed launch nonce. |
| `role_binding` | Role/profile/`can_spawn`/`depth` as committed. `role` absent means a role deliberately cleared. |
| `team_binding` | Team and parent as committed. `team_id` absent means unbound. |
| `launch_ready` | **Seals** the operation. |
| `restart_requested` | A restart was accepted; the relaunch opens a new epoch. |
| `rolled_back` | Authority withdrawn. The operation can never seal afterwards. |
| `closed` | Tab gone. |

Each transition is written by the code that commits the mutation, in the same
operation that performs it. Nothing observes tab state afterwards and infers a
transition from it.

## The recovery rule

An operation becomes a **checkpoint** only when all of this holds:

1. `launch_ready` sealed it, and
2. `provider_identity_committed` and `session_bound` both landed in that same
   operation, and
3. no `rolled_back` or `closed` followed.

Provider identity plus a nonce-proven session are the floor. A role or Team
binding without them is residue, never authority.

A snapshot's authority survives a restart only when:

- the tab claims authority at all (otherwise there is nothing to prove), and
- the journal replayed cleanly to its end, and
- a checkpoint exists for that exact `tab_id`, and
- the snapshot carries a lifecycle epoch **and** operation id, and
- both equal the checkpoint's, and
- every field the snapshot claims equals what the operation committed.

A snapshot may hold *less* than the checkpoint (a field cleared before
shutdown). It may never hold more, and never something different.

Otherwise the restore strips `agent_type`, `agent_session_id`, binding nonces,
`role`, `role_profile`, `team_id`, `parent_tab_id`, `can_spawn`, `depth`, boot
stage, agent state, and chatBackend binding, and logs the refusal:

```
[tab-lifecycle] tab db79d2da restored without authority: no committed lifecycle checkpoint for this tab
```

Refusal reasons are `no committed lifecycle checkpoint`, `no lifecycle
epoch/operation lineage`, `role authority claimed without a committed provider
identity and session`, `epoch mismatch`, `operation mismatch`, a named field
disagreement, or `journal tail refused`.

A role, Team, parent, `can_spawn`, or `depth` claim is refused outright unless
the same snapshot also claims the provider and session behind it. A role-bearing
plain shell is never restorable, however it came to be persisted.

**Never used as evidence:** a zero or null field, the tab title, the cwd, any
timestamp, temporal proximity, the newest provider session, or a session
belonging to another tab. A snapshot that predates the journal has no lineage
and is therefore unproven — that is the correct outcome, not a gap.

## Retention and compaction

The journal is bounded by explicit compaction, never by age:

- Compaction triggers automatically past 4096 records, and can be performed
  directly through `TabLifecycleJournal::compact`.
- It rewrites the file as one checkpoint per tab that has one, followed by the
  transitions belonging to operations no checkpoint covers.
- Closed tabs leave nothing behind.
- Replaying a compacted journal reaches exactly the checkpoints replaying the
  full history reaches.

The rewrite is atomic (temp file, then rename). Nothing else prunes the file,
and no reader may treat a missing record as permission to relax the rule.

## Inspecting it

Ordinary filesystem tools are the intended interface.

```bash
JOURNAL=~/.codeterm-dev/sessions/tab-lifecycle.jsonl

# everything that ever happened to one tab, in order
grep '"tab_id":"db79d2da"' "$JOURNAL"

# just the transition names and when they landed
grep '"tab_id":"db79d2da"' "$JOURNAL" \
  | jq -r '.entry | "\(.seq)  \(.recorded_at)  \(.transition.transition)"'

# does this tab have a committed checkpoint at all?
jq -c 'select(.record=="checkpoint" and .checkpoint.key.tab_id=="db79d2da")' "$JOURNAL"

# which tabs reached launch_ready without ever binding a session
jq -r 'select(.record=="transition") | .entry
       | "\(.key.tab_id) \(.transition.transition)"' "$JOURNAL" \
  | sort -u

# what the snapshot claims, next to what the journal proves
jq -r '.workspaces[].tabs[].split_tree
       | select(.agent_type != null or .role != null)
       | "\(.tab_id) role=\(.role) provider=\(.agent_type) epoch=\(.agent_identity.lifecycle_epoch)"' \
  ~/.codeterm-dev/sessions/last.json
```

Rotated snapshots in `sessions/backups/` are plain JSON and diff the same way.

In-process, `TabAudit::read` returns the same view as pure data (transitions,
checkpoints, the one committed checkpoint, tail refusal) so a future
`codeterm tab audit <id>` surface can render it without recovery depending on
any presentation layer.

## Not in scope yet

- **Tab creation** is not journaled. Creation confers no authority, and the
  first `epoch_opened` already marks where a lifecycle begins.
- **Rollback to a named checkpoint** is not implemented. Restoring a tab to an
  earlier committed checkpoint needs an in-process authority boundary that
  validates and applies it atomically and journals itself; it is deliberately
  not a file edit and not a CLI wrapper over one.

<!-- revision: 1 -->
