# Team view as mission control — take B

Artboards: `design/team-mission-control-take-b.html`. One file, six parts. The
toggle at top right switches between the two shipped themes.

- **Part 1 · The Board** — A1 live plan, A2 no plan.
- **Part 2 · The Tasks card menu** — B1 kanban with an illegal drag, B2 the menu
  across five card states plus both confirm weights, B3 the same menu inside the
  expanded task.
- **Part 3 · The Board menu** — C1 in four plan states, C2 clear-everything in
  two weights, C3 the approve transition.
- **Part 4 · Behind the cost door** — D1 as it opens, D2 with the depth expanded.
- **Part 5 · Options to choose between** — R1 to R5, numbered.
- **Part 6 · Phone** — F1.

## The governing rule

Simple by default, rewarding to dig. The default surface answers "is anything
wrong, and what do I do" in seconds and in words. Every figure on it is a door,
and the depth behind it is real: the tasks behind a progress bar, the rows behind
an aggregate, the distribution behind a median, the events behind a percentage.
The rail is the density ceiling, not a starting point — each cell gets one
headline, the smallest visual that carries it, and one link.

Four levels exist and each one is a deliberate step down: the rail cell, the
ledger's findings, the evidence disclosures, and finally the event rows
themselves. Nothing is hidden that a person would need at the level above it.

## Grounding: where every figure comes from

Bucket 1 is available now. Bucket 2 is derivable from data that exists but needs
a small read route. Bucket 3 is not drawn.

**Bucket 1 — available now, drawn freely.**

| Figure | Source |
| --- | --- |
| Task name, status, priority, type, handle | `Task` records on the plan |
| Who holds a task | `task.assigned_tab_id`, falling back to `task.lane.tab_id` |
| Agent role, provider, model, live state | `Tab.role_profile`, `agent_type`, `agent_session_model`, `agent_state` |
| What an agent is saying right now | `agent_state.status_line`, `last_turn_summary`, `question` |
| Dwell time on a row | `task.status_changed_at`, `updated_at`, `created_at`, plus milestone timestamps |
| Stalled, unstaffed, tab-gone flags | `aged_open_tasks` and `detect_drift` in `codeterm-plan/src/plan/reconcile.rs` |
| Presentation label and named blockers | `TaskPresentation` and `DependencyWait` in `service_plan/state.rs` |
| Plan title, id, planning state, section count | `Plan` record |
| Progress split by status | counted from `plan.tasks` |
| Bounce and accept counts on a finished task | `task.execution_attempts` |
| Spec section count and prose | `fetchSpecSections`, already wired in the Board view |
| Skills, slug, purpose, verdict, ledger edits | `GET /api/plans/:id/skills`, already wired |
| Review capacity (option R4-b) | `plan review status` — WIP count, limit, blocked reason |
| Lane hygiene (option R4-a) | `plan lanes` — one row per open lane |
| Spec coverage (option R2-c) | `plan coverage` |

**Bucket 2 — cheap, and named.**

- **Everything on the cost cell and the ledger page.** `aggregate_by_kind`
  already returns count, total, p50 and p95 bytes; `aggregate_by_plan` returns
  count and total bytes; `list_for_plan` returns the rows. Nothing exposes any of
  it. The work is one read route over `KpiLedger`. This is the known bucket-2
  item and it is the largest piece of work the design depends on.
- **Per-plan token totals.** `aggregate_by_plan` sums bytes but not tokens, so
  the token figure needs `list_for_plan` plus a client-side sum, or one extra
  SQL aggregate. Small, but it is not free.
- **The team-level skills union.** The skills API is plan-scoped. A team can hold
  more than one plan, so "skills this team wrote" is a union across the team's
  plans — one loop over `plan.team_id`, no new storage.
- **Clear everything.** `PlanStore::archive_and_detach_team_plans` already does
  exactly what the confirm describes. It is currently only invoked on team close
  (`service/team.rs`), so exposing it as a deliberate Board action needs a route
  and a command, not new behaviour.

**Bucket 3 — not drawn, and why.**

- **Money.** There is no price table anywhere in this product. Cost is stated in
  tokens, labelled tokens.
- **Skill usage.** The best headline for the skills cell would be "read 14 times
  this week." Nothing records a skill being read. Drawn as R3-c, explicitly
  marked as not drawable, and left off the real rail. *Recommendation: a
  `skill_read` event on the existing ledger would be a few lines and would turn
  the weakest cell on the surface into the most interesting one.*
- **Per-agent cost attribution as a headline.** The ledger has `tab_id` and
  `role` on every row, so it is computable — but it is behind the split-by
  control on the ledger page, never on the summary, because the expensive agent
  is usually the one doing the work.
- **Success rates, time-to-completion trends, throughput over time.** These need
  history the plan store does not retain in a queryable shape. Not drawn.

## Part 2 — the Tasks card menu

This is the piece that would have embarrassed the product if it were designed as
a whiteboard. A column is a lifecycle state and the daemon refuses illegal
writes. Every verb in the artboards is a real CLI verb, and every disabled item
carries the reason the daemon would actually give.

Verified in `service_plan/mutations/mod.rs`:

- **Entering `in_progress` requires a tab.** Without `assigned_tab_id` or
  `lane.tab_id`, the write fails with `InProgressRequiresTab`. A person dragging
  a card into In progress cannot make that true by saying so, so the column
  refuses the drop while the card is still in the air and offers the two verbs
  that would get it there: dispatch a worker, or assign a tab. The gesture ends
  in an action instead of a snap-back.
- **`in_review` is a one-way door.** Any plain status write out of review is
  refused with `TransitionDenied`, and the error itself names the remedy verb.
  So the review card's menu offers only Accept, Bounce and Hold — the three real
  verbs from `plan review` — and the move items are present but disabled with
  "Review is a one-way door."
- **Pausing from review is refused separately** with `ReviewHoldRequired`, so
  Pause is disabled on that card with "Hold is the review version of it."
- **Pausing is never a bare status write.** `TaskPauseRequiresReason` forces
  `plan task pause --reason`, so the menu collects the reason inside itself
  before the verb fires. Asking after the fact would be worse than asking first.
- **Blocking requires a label**, so the item reads "Mark blocked…" with the
  ellipsis that promises a prompt.
- **Done is rejected while subtasks are open**, and the error lists them by name.

Weight is carried three ways. A verb with consequences beyond a status change —
dispatch spawns a worker and provisions a lane, assign binds a tab, respawn
starts a fresh worker — gets a short accent mark and a one-line explanation, and
sits in its own group. A destructive verb sits alone under a rule and is the only
red thing in the menu. **Closing and removing are different and do not read the
same:** closing is a normal outcome with an optional result, and its confirm is a
plain one-sentence box; removing deletes the task, its milestones and its
attempts, and its confirm is a two-column ledger of what goes and what stays,
with a red border.

Keyboard behaviour is specified on artboard B2: `Shift+F10` opens from a focused
card, arrows skip disabled items, type-ahead jumps, `Esc` returns focus to the
kebab, and a confirm opens with focus on cancel so a held `Enter` cannot destroy
anything. Disabled items stay in the accessibility tree with `aria-disabled` so a
screen reader hears the reason rather than silence.

## Part 3 — the Board menu

Three items, two groups. Approve and Pause are ordinary verbs about a live plan;
Clear everything is destructive and sits under a rule, never adjacent to Approve
as if they were peers.

Both ordinary verbs are state-dependent. A drafting plan can be approved and
cannot be paused, because nothing is running yet. An approved plan cannot be
approved again, and the disabled item says when it was approved and where to
undo it. Pause requires a reason in this product — `plan pause --reason` — so
the reason is collected in the menu, and the copy carries the CLI's own promise
that task state and executor bindings are untouched. Switching plans is present
only when the team holds more than one; with one plan it is absent rather than
dead. **I kept Switch** because the plan is now the masthead that owns the list,
so the menu attached to it is the natural place to change which list you are
looking at; without it the only way to switch is to leave the Board.

**Clearing is not deleting**, and the confirm says so in plain words rather than
asking "are you sure". The honest answer comes from
`archive_and_detach_team_plans`: every plan bound to the team is archived, its
`team_id`, `owner` and `planner` are cleared, and a transfer record is written to
`owner_history`. Nothing is destroyed. So the confirm is two lists — what the
team loses, and what survives — and what survives is most of it: the plans, every
task on them, the Spec, the Plan, the skills, the agents' tabs, and the whole
cost ledger.

**Work in flight is the sharp edge and it leads.** Clearing while three workers
are mid-task is not a state transition, it is abandonment. That variant opens
with a red-railed alarm naming how many agents are working and how many tasks are
in flight, says their lanes and branches are left on disk, suggests stopping them
first, and swaps the button order so Cancel reads first. Clearing an idle team
and clearing a running one do not look the same.

One thing I could not verify and will not draw a confident confirm over: whether
detaching leaves the workers' `CODETERM_TASK_ID` bindings pointing at tasks in a
now-archived plan, and what the nudge scheduler does with them afterwards. The
confirm says the agents keep running and their work is not merged, which is true
from the plan store's side. The exact downstream behaviour of a live worker whose
plan was archived under it is worth checking before this ships.

**Approve is the one moment the surface changes character,** so C3 draws before
and after. Drafting shows fourteen tasks locked and a line saying nothing can be
dispatched; approved shows two started and twelve ready. The transition is one
gesture — the progress bar wipes in from the left over 620ms as the locked grey
splits — and the Dispatch verb on every pending card goes live at the same
moment. It runs once and never repeats, and it is off under
`prefers-reduced-motion`.

## Part 4 — behind the cost door

Reordered as asked. The page now opens with a sentence of judgement, then three
findings in plain words, each with one line of evidence underneath in mono. The
two numbers and the fourteen-day strip sit below the findings at a third of their
former height. "Where it went" and "Every event kind" are `<details>` elements,
closed on arrival; D2 shows them open. The per-kind table keeps every column —
count, median, p95 — and the by-kind/role/model/agent switch keeps working. A
table row opens the events behind it, which is the fourth and last level.

The ledger facts that shaped the design are unchanged and still verified:

- Token counts exist only on `token_usage` rows, so they are never folded into a
  byte total.
- On a `token_usage` row, `bytes` holds `input + output` **tokens**, because
  `KpiAttribution::token_event_at` passes `tokens.total()` as the byte count. The
  table labels those cells `tok`.
- `compaction` rows carry no tokens and zero bytes, so their size columns stay
  blank rather than reading `0 B`.
- Rows with a null plan id are a real bucket, not an error. This is also what
  keeps the no-plan Board honest: a team with no plan still has a cost.

## Part 5 — the options

Numbered so you can point at one. I did not vary by making the rail denser.

- **R1 · the cost cell.** R1-a is the one you praised, two numbers and a
  three-way strip. R1-b leads with spend, which is the least actionable number on
  the page. R1-c leads with the verdict in words, most faithful to the principle
  but with nothing to watch drift when things are fine. **My pick: R1-a.**
- **R2 · the documents cell.** R2-a keeps excerpts, R2-b is one line each, R2-c
  leads with spec coverage — "4 of 6 sections have a task against them" — which
  is the only variant that can be wrong in a way you would act on. **My pick:
  R2-c, with R2-b as the fallback if the coverage number proves noisy.**
- **R3 · the skills cell.** R3-a lists them, R3-b shows a count and the newest.
  R3-c is the one I would want and cannot draw. **My pick: R3-b**, because it
  holds its height as the skill count grows.
- **R4 · a fourth rail cell.** R4-a lane hygiene, R4-b review capacity, R4-c
  none. **My pick: R4-c, with R4-b appearing conditionally** only while review is
  actually full — it is the thing that explains why Dispatch is greyed out
  everywhere, and it is invisible noise the rest of the time.
- **R5 · the work list.** R5-a one ordered list, R5-b three quiet groups. R5-b
  lets the flag pills go, which is one fewer coloured thing per row, but three
  headings over four rows weighs more than the list. **My pick: R5-a.**

## On beauty

The craft rules this surface holds itself to, so they can be checked rather than
asserted:

One spacing scale and one type scale, both declared as variables at the top of
the stylesheet; nothing on the surface is a number somebody eyeballed. One
contained element — the task row — so hierarchy comes from containment rather
than from six cards with the same radius. Shadows appear only on things that
genuinely float, which is menus and confirms, and never on content. Colour is
rationed to one meaning each: accent for interaction and direction, amber for
attention, green for delivered, red for destructive, and nothing decorative.

The one deliberate subtraction this pass: the task row used to carry its status
in both a coloured left rail and a coloured presentation label. The label is now
plain muted text and only an attention flag is allowed colour, so one signal
carries one meaning.

Motion is two things and no more. A task a live agent is working breathes on its
left edge, at 2.6s, which means tokens are being spent on that row right now. The
approve transition wipes once. Both are off under `prefers-reduced-motion`.

The empty state is not elegance by subtraction. It is a place: it says the team
is here and just isn't pointed at anything, it names what survived the clear, it
names the Planner who would draft the next plan, and it offers the way back. Its
rail still carries real numbers, because the cost ledger does not empty when a
plan is detached.
