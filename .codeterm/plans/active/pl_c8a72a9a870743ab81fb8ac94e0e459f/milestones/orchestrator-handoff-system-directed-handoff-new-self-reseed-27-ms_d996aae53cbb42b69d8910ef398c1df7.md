---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_d996aae53cbb42b69d8910ef398c1df7
handle: orchestrator-handoff-system-directed-handoff-new-self-reseed-27-ms_d996aae53cbb42b69d8910ef398c1df7
title: decision
---

HANDOFF /new reseed (codex): daemon txn creates element+milestone → records handoff_reseed pending row keyed {pane,handoff,source_session} → sends provider fresh-context cmd (/new via CAPABILITY not hardcoded) → on SessionStart new session id → sends EXACTLY ONE resume message. Monotonic states created→new_sent→session_seen→resume_sent→done, idempotent, loop-guard via handoff_resume_of metadata + --force. Resume prompt = short imperative pointing at 'codeterm plan handoff show <p> <h> --markdown' + agents.md + resume from Next Actions. --dry-run shows send sequence. compact-with-handoff = same artifact + provider compact cmd, weaker (spends tokens/stale focus); /new is default. Canvas: reset/branch glyph in step thread + orchestrator-group 'context reset' timeline spine, expand via worlds content-LOD, provenance chips old→new pane/session.
