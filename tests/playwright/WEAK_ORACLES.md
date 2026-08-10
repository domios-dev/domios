# Playwright weak-oracle disposition

Scope: Phase 1 hygiene sweep for specs where `toBeVisible()` / `toBeTruthy()` was the only final oracle.

| Category | Count | Disposition |
| --- | ---: | --- |
| Sole weak final assertion before sweep | 7 | Scanner listed tests with exactly one `expect()` and final matcher `toBeVisible` / `toBeTruthy`. |
| Strengthened in-place | 6 | Added content, count, or identity assertions after the visibility check. |
| Justified residue | 1 | `tests/playwright/suites/19-web-stability/web-stability.spec.ts` PAR-1/MEM keeps `memory-tabl` visibility plus `expectNoRuntimeErrors(errors)`, which is the real behavioral oracle but is helper-wrapped rather than a direct `expect()`. |

Strengthened tests:

- `12-web/chat-image-paste.spec.ts` now checks the attachment chip content.
- `12-web/web-parity.spec.ts` now checks workspace item count after mobile sidebar reopen.
- `12-web/web.spec.ts` now checks bad-token error text.
- `13-canvas/plan-card-metadata.spec.ts` now checks list rows after switching mode.
- `15-extensions/extensions.spec.ts` now checks broken-channel error text.
- `17-open-shell/lmstudio.spec.ts` now checks tool-result text.
- `19-web-stability/web-stability.spec.ts` GRP-4 now checks the visible terminal belongs to the expected tab.

## Not covered at the Playwright layer

| Surface | Why the Playwright layer cannot assert it | Where it is covered instead |
| --- | --- | --- |
| Child tab reordering within a parent group (`18-reorder/reorder.spec.ts`, removed C2) | The Playwright layer cannot construct a parent/child tab pair without a live agent. `parent_tab_id` is settable only through `POST /tabs/{id}/role` or `POST /tabs/{id}/make-orchestrator`, both of which transition with `require_verified_session: true` and answer `400 agent_unavailable` for a tab that never reported a session binding, or through `POST /agents/spawn`, which spawns a real provider agent — and the `golden` project grep-inverts `@stub`. The UI handle is gone as well: only `tab:<workspaceId>` and `workspace:workspaces` dnd groups render, so `child:<parentTabId>` cannot be dragged. | `src-tauri/crates/codeterm-pty/src/multiplexer/state/reorder.rs`: `reorder_children_orders_only_same_parent_siblings`, `reorder_children_rejects_foreign_parent`, `reorder_tabs_rejects_a_child_tab`. |
