---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_e9a1038bbd4a4e96b03ec8c579554585
handle: shared-primitives-real-button-chip-card-components-grep-gates
title: 'Shared primitives: real Button/Chip/Card components + grep-gates'
status: done
---

Turn the class-string tokens into REAL components and make bypassing them impossible. (1) <Button> in shared/ wrapping the UI_BUTTON variants (primary/ghost/accent/danger/compact/icon/sm/lg) with size/variant props, focus-visible ring, disabled/loading states (loading reuses BusyState dots); (2) <Chip>/<Badge> (status colors from the existing status modules), <Card> (radius/border/padding tokens), <FormRow> (label+control+help grammar from Settings); (3) fill the gap list from the audit task; (4) a /components gallery route (dev-only, like the mission-control prototype gallery) rendering every primitive x variant x state for eyeball QA and Playwright screenshots; (5) GREP-GATES per the BusyState.grepgate.test.ts precedent: new tests failing on bespoke button/chip markup outside shared/ (allowlist = the audit table's 'genuinely custom' entries, baseline-locked, only shrinks — same mechanics as the file-size gate). Executable test plan: component tests per primitive (variants/states/a11y), gallery Playwright screenshot per primitive, grepgate tests red on a planted bespoke button fixture then green after removal.
