# Playwright weak-oracle disposition

Scope: Phase 1 hygiene sweep for specs where `toBeVisible()` / `toBeTruthy()` was the only final oracle.

| Category | Count | Disposition |
| --- | ---: | --- |
| Sole weak final assertion before sweep | 7 | Scanner listed tests with exactly one `expect()` and final matcher `toBeVisible` / `toBeTruthy`. |
| Strengthened in-place | 6 | Added content, count, or identity assertions after the visibility check. |
| Justified residue | 1 | `tests/playwright/suites/19-web-stability/web-stability.spec.ts` PAR-1/MEM keeps `memory-panel` visibility plus `expectNoRuntimeErrors(errors)`, which is the real behavioral oracle but is helper-wrapped rather than a direct `expect()`. |

Strengthened tests:

- `12-web/chat-image-paste.spec.ts` now checks the attachment chip content.
- `12-web/web-parity.spec.ts` now checks workspace item count after mobile sidebar reopen.
- `12-web/web.spec.ts` now checks bad-token error text.
- `13-canvas/plan-card-metadata.spec.ts` now checks list rows after switching mode.
- `15-extensions/extensions.spec.ts` now checks broken-channel error text.
- `17-open-shell/lmstudio.spec.ts` now checks tool-result text.
- `19-web-stability/web-stability.spec.ts` GRP-4 now checks the visible terminal belongs to the expected tab.
