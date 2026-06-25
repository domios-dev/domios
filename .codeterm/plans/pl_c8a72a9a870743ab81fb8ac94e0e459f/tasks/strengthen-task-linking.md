---
tags:
- cli
---

# Strengthen task↔section linking (advisory hint is ignored) + backfill 35 unlinked

LIVE FACT: dogfood plan pl_c8a72a9a has 35/91 tasks with NO section link. The cli-guardrail hint DOES fire on `plan task add` ("task is not linked to a plan section… connect it --sections <id>") but it is ADVISORY — it was ignored 35 times. A hint that doesn't change behavior isn't enough.

MAKE LINKING ACTUALLY HAPPEN (pick the right mix, don't over-force):
1. EASIER: `plan task add` should accept/suggest a section inline and, when a plan has an obvious target group/section, AUTO-LINK or propose the best match (fuzzy by name/group) so the caller confirms instead of hand-copying ids. Andrey: "CLI should always help link."
2. STRONGER SIGNAL: make the unlinked state visible+actionable — `plan gaps`/`plan get` should surface "N unlinked tasks" prominently; a `plan relink`/`plan task link` quick path; optionally a periodic audit reminder (ties to orch-doc-reminders notifiers).
3. BACKFILL: assign the 35 existing unlinked tasks to their right sections (by group/topic). This is a data pass on the dogfood.

Decide whether linkage should ever be REQUIRED (some cross-cutting tasks legitimately have none) — prefer strong-default + easy-link over hard-block.
