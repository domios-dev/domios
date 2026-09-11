# Team view as mission control — take B

Artboards: `design/team-mission-control-take-b.html`. Open it in a browser; the
toggle at top right switches between the two shipped themes.

Six artboards: Board with a plan, Board with no plan, the crew cell annotated
with the field behind every state, the cost cell in full, both overflow menus
plus the destructive confirm, and phone width.

## The position

**One thing dominates: the crew.** It takes the full height of the page and
roughly seventy percent of its width, and it is the only surface on the page
that is contained — a border, a fill, a radius. Everything else is a hairline
rule in a 336px rail. There are no cards in the rail. Hierarchy comes from
containment and weight, not from six boxes with the same corner radius asking
to be read in an unspecified order.

The reason is the question people actually arrive with. A team is a crew of
agents burning tokens while you are somewhere else. You open it to learn
whether the machine is running and whether anything needs you. That is a
checking posture, not a reading posture, and it repeats forty times a day.
Everything else on the page — what the team produced, what it wrote down, what
it cost — is a retrospective question asked perhaps once a day. Retrospective
content belongs in a rail, reachable and quiet. It does not deserve equal area
with the thing you came for.

**The crew rows are where the boldness is spent.** Each row carries a live
line: the provider's own status text while an agent works, its last-turn
summary while it sits idle, its question while it waits on you. That live line
is what makes the page feel inhabited instead of inventoried. Every other
element on the page is deliberately quiet so that one can be loud.

**One motion, one meaning.** A working row's left rail breathes on a 2.6s
cycle. Nothing else on the page moves. A breathing rail means tokens are being
spent on that row right now, and because it is the only animation, it reads
instantly instead of competing with hover transitions on six cards. It falls
back to solid under `prefers-reduced-motion`.

## Three disagreements with the brief

**1. Agents and tasks are one list, not two cells.** The brief asks for an
agents cell and, separately, for "recent and in-progress tasks, each showing the
agent assigned to it." Those are the same rows read from opposite ends. Build
both and every in-flight task appears twice on one screen, with two different
visual treatments, and the reader has to work out that they are the same thing.

So there is one work list. A staffed row is agent-first: role, handle, model,
state, what it is doing, and the task it holds as a chip. An unstaffed row is
the same shell read from the task end: "Nobody assigned," the task id, its
priority and type, how long it has sat, and what it needs. Merging them makes
the most important negative fact on the page visible for free — a task nobody
is working on sits in the same column as the agents, in the same visual
grammar, impossible to miss. In a separate tasks cell it would be one row among
many.

Finished tasks and closed agents collapse behind one line at the bottom of the
list. They are history; history is a click.

**2. There is no money in this product, so "cost" is tokens.** The brief asks
what the team cost. The ledger has no price table — none exists anywhere in the
repository — so any currency figure would be invented. The headline number is
`4.20M tokens`, labelled tokens.

More importantly, the token total is not actionable. You cannot do anything
differently because a number is 4.2M rather than 3.8M. What the ledger's kind
vocabulary makes available, and what a person can act on, is the shape of the
traffic. So the second headline is **chasing**: the share of events that were
`nudge`, `poll`, `heartbeat`, `bounce` or `compaction` — the crew checking on
itself and sending work back rather than delivering it. When that share climbs,
something is stalled or a worker is producing work that keeps bouncing. That is
a number you act on. It sits beside the token total at equal weight, and it is
the only number on the page drawn in the warning colour.

**3. The orientation paragraph is replaced by a standing report.** The brief
asks the surface to "open with a short line or two of orientation." A paragraph
explaining what a team view is gets read once and then costs eighty pixels
forever. The masthead instead carries a sentence assembled from live state:
"Five agents. Two are working, one is waiting on you, two are idle. One task has
sat pending for two hours with nobody on it." It orients a newcomer on the
first visit and reports status on every visit after. The clause that costs you
something by being missed is the one that takes the warning colour.

## The cost cell is drawn against the real table

Everything in artboard D comes from `kpi_events` as it exists in
`src-tauri/src/agent/kpi_ledger.rs`. The specifics that shaped the design:

- **Tokens live only on `token_usage` rows.** They are never folded into a byte
  total anywhere on the page.
- **On a `token_usage` row, `bytes` holds `input + output` tokens**, because
  `KpiAttribution::token_event_at` passes `tokens.total()` as the byte count.
  The per-kind table therefore labels that row's size columns `tok`, not `B`.
  Mixing that row into a megabyte sum would have produced a confidently wrong
  number.
- **`compaction` rows carry zero bytes and no tokens**, so their size cells are
  blank rather than reading `0 B`, which would imply a measurement.
- **`aggregate_by_kind` returns count, total, p50 and p95 bytes**, so the table
  shows exactly those columns. The p50/p95 pair is the most useful thing the
  ledger offers and nothing else surfaces it: a report whose p95 is fifteen
  times its median means workers are pasting whole files into reports instead
  of naming them. That reading is written out under "What to do about it,"
  because a percentile nobody interprets is decoration.
- **`aggregate_by_plan` returns count and total bytes per plan, with a null
  plan id as the real unattributable bucket.** It appears as "charged to no
  plan," italic and dimmed — a legitimate row, not an error. It is also what
  makes the no-plan artboard honest: a team with no plan still has a cost.
- **`role`, `model` and `tab_id` are columns on every row**, so the split-by
  control offers kind, role, model and agent. It does not offer anything the
  table cannot group by.

One caveat worth stating rather than hiding: `aggregate_by_plan` sums bytes but
not tokens, so a per-plan token figure needs `list_for_plan` and a client-side
sum. Both paths exist; the design does not depend on a query that does not.

## What I left out

**The side chat, entirely.** Removed, as asked, and I agree. A chat pane
narrow enough to fit beside a full page is worse at chatting than the tab it
duplicates, and it steals width from the one column that needed it.

**A big-number hero.** The default treatment for a dashboard is one enormous
figure with a gradient behind it. There is no single number here worth that
weight; the two that matter are a token count and a ratio, and they are the
same size as each other on purpose.

**Per-agent token attribution in the rail.** The ledger can produce it, but a
per-agent token bar invites optimising the wrong thing — the expensive agent is
usually the one doing the work. It is available behind the split-by control on
the ledger page, where a person has context, and nowhere on the summary.

**A flow diagram of the team.** Mission control already has one on the parked
canvas surface. A picture of the hierarchy answers a question nobody asks twice.

**Skills as metadata.** Treated as the brief asks: a titled section, "Skills
this team wrote," with each skill's title, slug, purpose, ledger verdict and
edit count, and a way through to browse them. What I did not do is give them
more room than that. Two or three skills is the common case; a section that
holds three rows does not deserve a third of the page.

## Controls

Every control sits on the object it changes, behind an unobtrusive overflow
that appears on hover and stays visible on phone. A task's menu opens on the
crew row that holds it. The plan's menu opens beside the plan title in the
rail, with **Approve the plan** first, because that is the item a person opens
that menu to press — approval is an action, not a badge. The chips say where
the plan stands; the menu is how it moves.

Destructive items are last, alone under a rule, and red. Nothing else in a menu
is red. Choosing one replaces the menu with a confirm that names what
disappears and what survives — "its three milestones are deleted, the plan keeps
its other thirteen tasks" — and both the confirm's action button and its border
are red. Two deliberate clicks, never one.

## The empty room

The brief's strongest complaint is that a team with no plan reads as broken.
Under this hierarchy that problem mostly dissolves, because the dominant
element does not depend on a plan existing. A team exists because agents exist,
so the crew column is full whether or not anyone has drafted a plan; artboard B
shows three idle agents each still saying what they last did, plus one whose tab
has closed. The rail's plan section becomes a single invitation with one primary
action. Cost still has real data, because events with a null plan id are
recorded and the copy says so plainly: work done before a plan exists moves
under the plan once one is created. The documents and skills sections say what
they will hold and who fills them.

The genuinely empty case — a team with no plan and no agents — is a team that
was created seconds ago. There the crew column carries the invitation instead
of the rail, and it is the only thing on the page.

## Phone

The ordering is the argument, compressed. The standing line and a one-row plan
strip answer "which plan, and does anything need me" above the fold. Then the
crew, sorted by claim on the user: waiting first, working next, idle and
unstaffed folded behind one line. The rail's four sections become accordion
rows with their summary value on the right, so "4.20M tok" and "9/14" are
readable without expanding anything. Documents, skills and the full ledger are
three taps that nobody takes while walking, which is the correct cost for them.

## Visual language

Nothing here invents a palette. The artboard's two `:root` blocks are the
literal output of `themeToCSS()` for the shipped github-dark and github-light
themes, and every surface is derived from those variables with `color-mix`, the
same way `globals.css` does it. Task status dots use the exact values from
`TASK_STATUS_COLORS`. Type is Inter and JetBrains Mono, the product's own
faces; mono is reserved for machine identity and numbers — handles, model ids,
plan and task ids, tabular counts — and never used as a decorative label face.
The switcher, the state chips and the kebab affordance follow the existing
`plan-board-switcher`, `plan-state-chip` and `plan-switcher-kebab` specs so the
new surface does not arrive looking like a different product.
