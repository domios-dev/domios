---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_5e0f59331cb6419796ae7014ffc7fe18
handle: build-honor-cargo-target-dir-when-copying-bundling-cli
title: 'BUILD: honor CARGO_TARGET_DIR when copying/bundling CLI'
status: done
tags:
- cli
- build
---

Fix the latent CLI packaging footgun found during release analysis: build.sh install_cli_to and scripts/bundle-cli.js must locate the cargo-built CLI under CARGO_TARGET_DIR when that env var is set, instead of hardcoding cli/target.
