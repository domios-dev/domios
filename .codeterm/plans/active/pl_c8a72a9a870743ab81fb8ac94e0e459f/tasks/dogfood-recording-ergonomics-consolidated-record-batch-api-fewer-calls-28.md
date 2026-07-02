---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_e4a8a77d4a994db5b08ea7ffa1dce365
handle: dogfood-recording-ergonomics-consolidated-record-batch-api-fewer-calls-28
title: 'Dogfood-recording ergonomics: consolidated record/batch API (fewer calls) §28'
status: done
---

ANDREY: recording dogfood today = MANY calls (POST /milestones xN + task create + body PATCH). Embed the ergonomic record surface into BOTH the CLI AND the app/daemon side (first-class, not an orchestrator-only workaround) so the app UI + all agents write provenance through the same convenient path. Deliver: (1) consolidated one-call record — codeterm plan record / codeterm dogfood that upserts a task + attaches N milestones + sets body + links docs, idempotent, returns ids; (2) a batch op endpoint POST /api/plans/:id/batch (array of ops) used by CLI AND in-app; (3) --from-file/stdin for bodies. In-process daemon API so native UI + drop-in agents reuse it. Smallest surface, most round-trips removed, structured provenance preserved. Composes with handoff (a handoff IS a dogfood write). Design §28.
