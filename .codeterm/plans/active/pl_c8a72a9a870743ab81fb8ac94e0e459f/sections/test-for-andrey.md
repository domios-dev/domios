---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: section
id: sec_8a4774e4ba784041aefb1ca930a22e84
handle: test-for-andrey
title: Test For Andrey
---

**HOW live-mutability is smoke-tested end to end.** This is a throwaway probe, kept honest about being one. It exercises the full live-update loop with no code — just a section write — and a human oracle: if this section appears on the SPEC node (and its row on the PLAN node) **without a reload**, the chain works.

The chain under test, all real and documented elsewhere in this plan:

```
plan section update --body-file  →  write_atomic (temp + rename)
  →  fs notify debouncer (200ms)  →  watcher skips is_own_recent_write? NO (external CLI)
  →  reload_from_disk(plan_id)    →  emit_everywhere("plan-changed", {plan_id})
  →  PlansProvider re-fetch        →  layoutWorkspace re-projects  →  canvas repaints
```

The body text revealing as you zoom in is the second assertion — it confirms the W2 content band (`worlds.ts`) actually surfaces body markdown, not just titles.

## Key decisions
- **A real section as the test, not a unit mock** — it proves the *integration* (watcher → event → provider → layout → paint), which unit tests can't.
- **Human-visible oracle** — "did it appear without reload" is the exact UX promise; an automated version of this is the Playwright live-update spec.
- **Disposable by design** — safe to remove with `codeterm plan section rm`; it carries no implementation, only verification intent.
