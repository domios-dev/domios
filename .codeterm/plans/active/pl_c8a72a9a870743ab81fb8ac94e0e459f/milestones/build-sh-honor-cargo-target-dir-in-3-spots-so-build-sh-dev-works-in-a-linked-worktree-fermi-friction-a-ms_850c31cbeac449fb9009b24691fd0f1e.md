---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_850c31cbeac449fb9009b24691fd0f1e
handle: build-sh-honor-cargo-target-dir-in-3-spots-so-build-sh-dev-works-in-a-linked-worktree-fermi-friction-a-ms_850c31cbeac449fb9009b24691fd0f1e
title: testing
---

ACCEPTED on review. build.sh release_dir() helper (CARGO_TARGET_DIR/release else fallback) for CLI cp; ensure_dist_placeholder() seeds dist/index.html for fresh worktree (vite serves live UI); bundle-embed.js honors CARGO_TARGET_DIR for the embed sidecar. ALL 3 fall back to defaults when CARGO_TARGET_DIR unset -> prod/main path UNCHANGED. bash -n + node --check clean. @4b0c7e5e on canvas/buildsh-targetdir (off iso-dev-data). Fixes Fermi friction A; behavioral proof = next worktree dev launches clean (deferred, no risk to defaults).
