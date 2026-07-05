---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_691cc849a1114b01a4f5393c00dc9dba
handle: shared-primitives-real-button-chip-card-components-grep-gates-ms_691cc849a1114b01a4f5393c00dc9dba
title: status_update
---

Accepted and merged in 25ada2f3 (feat(shared): add design primitives and gallery gate).

Integration changes beyond worker commit: removed comment-hygiene violations, changed gallery route to match the existing prototype pattern (query-gated without import.meta.env.DEV so it works in CodeTerm dev's built bundle), kept gallery component lazy-loaded.

Gates run in warm checkout:
- npx tsx Button.test.tsx: 9/9 pass
- npx tsx Chip.test.tsx: 6/6 pass
- npx tsx Card.test.tsx: 6/6 pass
- npx tsx FormRow.test.tsx: 6/6 pass
- npx tsx shared.grepgate.test.ts: pass, baseline locked chip=23/38 and button=16/26
- npx tsx BusyState.grepgate.test.ts: pass
- npx tsc --noEmit: pass
- pnpm build: pass, ComponentGallery emitted as a lazy chunk
- Playwright smoke via @playwright/test: ?prototype=gallery rendered [data-testid=component-gallery], sections present, screenshot saved to /tmp/component-gallery.png
- git diff --check, file-size gate, comment-block gate: pass
- CODETERM_API_PORT=7686 ./build.sh check: pass
- pre-push cargo check/fmt/clippy: pass

Follow-up remains task_0a80b6ed: migrate surfaces onto the primitives; grepgate baselines must only shrink.
