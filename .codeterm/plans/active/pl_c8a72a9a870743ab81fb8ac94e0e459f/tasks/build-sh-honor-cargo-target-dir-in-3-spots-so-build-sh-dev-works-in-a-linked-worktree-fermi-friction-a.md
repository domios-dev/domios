---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_5480af97936e446aa77c1aafacb6292a
handle: build-sh-honor-cargo-target-dir-in-3-spots-so-build-sh-dev-works-in-a-linked-worktree-fermi-friction-a
title: 'build.sh: honor CARGO_TARGET_DIR in 3 spots so ''build.sh dev'' works in a linked worktree (Fermi friction A)'
status: done
tags:
- friction
---

Fermi hit 3 hardcoded-path breakages that abort 'build.sh dev' in a fresh linked worktree when CARGO_TARGET_DIR points at the warm shared target: (1) install_cli_to copies cli/target/release/codeterm but CLI lands in CARGO_TARGET_DIR; (2) tauri build-script needs ../dist which a fresh worktree lacks; (3) ensure_real_embed_bundle/bundle-embed.js looks for codeterm-embed in the worktree-local target not CARGO_TARGET_DIR. Builds on Hopper's worktree-local-data change. Fix: honor CARGO_TARGET_DIR in those 3 spots so isolated worktree verification dev works out of the box.
