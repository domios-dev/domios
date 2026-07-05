---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_85791d257b844b388645ccebffe33437
handle: isolated-dev-in-worktree-ms_85791d257b844b388645ccebffe33437
title: decision
---

ANDREY: CODETERM_INSTANCE today puts data in ~/.codeterm-dev-<name> (clutters users folder). Want option to keep the isolated instance data INSIDE the worktree as e.g. .codeterm-isolated-<name>/ (gitignored, lives in project, never in git, doesn't pollute ~/users). Main ~/.codeterm-dev stays in users folder = fine. Add a flag/convention so build.sh dev with an in-worktree instance writes there + auto-gitignore.
