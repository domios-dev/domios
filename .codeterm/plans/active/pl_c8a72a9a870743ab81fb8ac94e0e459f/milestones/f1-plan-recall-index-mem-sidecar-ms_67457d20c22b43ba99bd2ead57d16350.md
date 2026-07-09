---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_67457d20c22b43ba99bd2ead57d16350
handle: f1-plan-recall-index-mem-sidecar-ms_67457d20c22b43ba99bd2ead57d16350
title: testing
---

F1 ACCEPTANCE PASSED on the real dogfood plan (7686, build 2b0be830): plan reindex + mem search 'plan button soft-lock' --project returned the soft-lock task as source=plan (PLAN HITS:1). Search→index path fixed (overfetch + preserve plan hit), plan reindex added, CLI socket footgun fixed (explicit non-prod CODETERM_API_PORT wins over inherited prod CODETERM_SOCKET_PATH). The dogfood loop closed: bug found via dogfood acceptance → fixed → verified on the live plan.
