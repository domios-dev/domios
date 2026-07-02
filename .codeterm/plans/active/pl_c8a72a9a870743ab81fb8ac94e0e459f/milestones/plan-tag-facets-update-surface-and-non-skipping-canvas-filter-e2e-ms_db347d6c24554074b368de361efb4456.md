---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_db347d6c24554074b368de361efb4456
handle: plan-tag-facets-update-surface-and-non-skipping-canvas-filter-e2e-ms_db347d6c24554074b368de361efb4456
title: status_update
---

Integrated tag-facet update surface and non-skipping canvas filter e2e. Verified in hot checkout: cargo fmt --check for cli/src-tauri, cargo test cli plan_facets_set_parses, cargo test src-tauri tag_facets_from_body --lib, cargo test src-tauri set_tag_facets_persists_and_reloads --lib, npx tsc --noEmit, CT_E2E_PORT=7686 Playwright @canvas-tag-filter 1/1, CODETERM_API_PORT=7686 ./build.sh check, git diff --check.
