---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_14aac809e1b4480b87508fbe4c178513
handle: plan-views-back-navigation-for-cross-surface-jumps-ms_14aac809e1b4480b87508fbe4c178513
title: review
---

BOUNCE — plan-back-nav lane. Tests pass (navStack 10/10, planNavBack 9/9 via npx tsx; check-file-sizes.sh clean, files <1000). BUG: PlanDetailOverlay onKeyDown useCallback deps=[onClose] omits canGoBack+onBack; overlay mounts once with canGoBack=false and never remounts across jumps (onClose stable), so keyboard listener keeps stale canGoBack=false -> Alt/Cmd+Left silently dead after navigation; only mouse button works. Violates 'back button (and Alt/Cmd+Left)' criterion. Tests miss it (only fresh-mount canGoBack=true). GOOD: navStack pure/capped/immutable, reset-on-plan-switch present, NO history loop (handleBack no re-push), Esc unchanged, a11y OK. FIX: add canGoBack,onBack to onKeyDown deps + test false->true transition. GATE: bounce to worker pane 8631b060.
