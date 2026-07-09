---
title: Gap analysis — verify-before-claim audit of a plan before execution
area: planning
tags: [planning, review, verify-before-claim, pre-execution]
---

# Gap analysis — pre-execution audit of a CodeTerm plan

Run this AFTER a plan's structure is cut (GOAL + Spec + Plan sections + Tasks
with briefs) and BEFORE the Orchestrator dispatches any lane. It is the stage
between `structure_ready` and `approved`.

`codeterm plan doctor <plan> --kind quality` checks plan *structure* — coverage,
vague briefs, missing acceptance/test-plan. It CANNOT check plan-vs-codebase
*truth*: whether the tables/services/fields a task assumes actually exist, or
already exist. That gap is what this skill closes. In real runs it has caught
CRITICAL flaws doctor is structurally blind to (tasks that rebuild things that
already exist, enforcement seams keyed on an engine that isn't there, naming
collisions with an existing domain).

## When to use

- Spec + Plan + Tasks exist, no lane dispatched yet.
- User asks "review the plan", "what could go wrong", "check the plan against
  the codebase", "перед стартом проверь план", or any pre-execution audit.

Do NOT use mid-execution (fix inline then) or while structure is still being cut
(that is planning, not audit).

## Method

### 1. Read the whole plan, list every structural claim

```bash
codeterm plan get <plan> --json        # or --sections for cheap structure
codeterm task show <task_id>           # per task, cheap
```

Note every concrete claim the tasks make: file paths created/modified, model
fields referenced, service chokepoints assumed single, existing patterns
emulated, migrations, tables. These are your verification targets.

### 2. Verify EACH claim against the codebase — never from memory

For every claim, prove it in code before writing it as a finding:

- File paths the plan creates — do the parent dirs exist? Is it already there?
- Fields/types referenced — do they exist with that exact name/type? (SCIP index
  or grep + read — see AGENTS.md "Code navigation").
- "Single entry point" / "chokepoint" claims — are there bypass paths?
- Patterns the plan emulates — read 1-2 real examples.

If you cannot grep it, you cannot claim it. Build a **Verified facts** table
(`claim | ✅ file:line`). It signals what you actually checked vs assumed and
stops you writing findings that contradict reality.

### 3. Categorize findings by severity (honestly)

- **CRITICAL** — plan fails its own tests / builds duplicate of existing code /
  corrupts data. BLOCKS execution; fix the plan first.
- **HIGH** — bites during execution (races, O(N²) at prod scale, fixture side
  effects). Surface for a fix-now-or-monitor call.
- **MEDIUM** — design choice a reader should know; can defer.
- **LOW / DEFER** — explicit out-of-scope; acknowledge so it is not forgotten.

### 4. Each finding: concrete, with a fix

`Where` (`file:line` or `Task N`) · `Problem` (show the broken case) · `Fix`
(the specific change / diff) · `Severity`. Be specific about cost ("14 GB RAM",
not "expensive"). No "might" / "could" — verify, then state.

### 5. Surface design decisions as Open questions

Frame both options + a recommendation + the consequence of each. The user
decides; you apply and mark RESOLVED.

### 6. It is a living document

Keep a status board (Verdict + pending) and an action-items table. Mark items
RESOLVED / NO-LONGER-APPLICABLE / DEFERRED / NOTED — never delete history.
Fold resolved findings into the plan as tasks or edits; the audit is commentary
on the plan, not a substitute for it.

## Exit gate

Execution starts only when every CRITICAL is RESOLVED. HIGH/MEDIUM/LOW may stay
pending with awareness, or become new tasks (`codeterm plan task add`).

## Bundle as a plan skill

To make this run on a specific plan for every executor/QA:

```bash
codeterm plan skill new gap-analysis    # scaffolds the folder for this plan
codeterm plan skill show gap-analysis
```
