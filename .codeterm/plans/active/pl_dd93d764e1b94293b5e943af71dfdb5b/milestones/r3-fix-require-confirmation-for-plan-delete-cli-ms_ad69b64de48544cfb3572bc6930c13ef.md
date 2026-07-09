---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_ad69b64de48544cfb3572bc6930c13ef
handle: r3-fix-require-confirmation-for-plan-delete-cli-ms_ad69b64de48544cfb3572bc6930c13ef
title: status_update
---

Accepted and merged through ce525b51. CLI plan delete now requires --yes (hidden --confirm alias), refuses locally before socket/RPC when missing, archive/unarchive unchanged. Gates: plan_delete_refuses_without_confirmation_before_rpc PASS; plan_delete_confirmation_builds_rpc_params PASS; plan_delete_confirmation_flags_parse PASS under scripts/test-capped.sh. Broad subcommand_contract filter timed out on unrelated existing parse tests, replaced with exact new parse test. Pre-push cargo check/fmt/clippy PASS.
