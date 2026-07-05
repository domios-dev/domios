---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_095cc9a841f046bbb2bb7820267ccdbc
handle: isolated-dev-in-worktree-ms_095cc9a841f046bbb2bb7820267ccdbc
title: testing
---

ACCEPTED on review+behavior gate. build.sh worktree-detection VERIFIED empirically: main checkout (git-dir==git-common-dir) -> ~/.codeterm-dev UNCHANGED (running 7686+prod safe); linked worktrees -> <root>/.codeterm-dev local. paths.rs data_dir_override (absolute-only CODETERM_DATA_DIR) + data_root + config_path reviewed correct; general_agent routes through data_root; .gitignore updated. Full cargo compile deferred to mainline (worktree binary build-script needs bundled-embed resource = env artifact, not code). @8a1ad97 on canvas/iso-dev-data. This unblocks self-contained worktree verification instances.
