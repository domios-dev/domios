---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_6468a979f8864e369c8c600f8725eed1
handle: gh-182-180-agent-pane-lifecycle-lazy-materialization-send-partial-restar
title: 'GH #182 + #180: Agent pane lifecycle — lazy materialization send + partial restart'
status: done
---

Two lifecycle issues, one lane. STEP 1: gh issue view 182 && gh issue view 180. (A) #182: 'codeterm send' to a lazy/not-yet-materialized agent pane delivers the text to the RAW SHELL instead of the agent (the provider process hasn't claimed the PTY yet). The restore-matrix eval (evals/restore-matrix/) has a pre-kill materialization gate — reuse its oracle. Fix: sends to an agent pane must wait for (or trigger) materialization before delivery, with a bounded timeout + honest error, never silent shell delivery. NOTE: may interact with the wake-confirmer lane (task_fff79b09) — coordinate, do not duplicate. (B) #180: agent restart can partially restart a pane — sandbox/profile changes but the session does not (stale session with new flags); make restart atomic: either full new session with new profile or clean resume, never a mix. If already fixed on branch, close with evidence.
