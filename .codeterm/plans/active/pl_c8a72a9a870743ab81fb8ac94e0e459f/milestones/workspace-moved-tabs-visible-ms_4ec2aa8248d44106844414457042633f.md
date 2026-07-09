---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_4ec2aa8248d44106844414457042633f
handle: workspace-moved-tabs-visible-ms_4ec2aa8248d44106844414457042633f
title: status_update
---

Accepted workspace-moved-tabs: integrated sidebar cross-workspace child visibility, explicit --workspace precedence over parent for spawn, and workspace.move_tab state emission through service. Verified hot checkout: npx tsx src/components/Sidebar/workspaceTabs.test.ts passed; cargo test --manifest-path src-tauri/Cargo.toml resolve_spawn_workspace_prefers_explicit_workspace_over_parent passed; npm run build passed. Manual UI verify remains after safe dev restart.
