---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_95bbdabeebb34c5ba1a1814bb3ee8305
handle: gh-209-linux-updater-fails-invalid-updater-binary-format-release-blocker
title: 'GH #209: Linux updater fails — invalid updater binary format (RELEASE BLOCKER)'
status: done
---

STEP 1: gh issue view 209 — the issue body is the spec. Linux updater rejects the updater binary format. This DIRECTLY blocks the upcoming release (Linux .deb/.AppImage ship from the farm). Reproduce against the latest published release artifacts + the branch's updater code (src-tauri updater config, tauri.conf, release workflows in .github/workflows/release-linux.yml). Likely area: updater artifact naming/signature format mismatch between what release-linux.yml publishes and what the tauri updater expects (.tar.gz vs AppImage, signature file naming). Fix so a Linux install can self-update from the previous release to the next one. Coordinate with the ubunta regression lane for a live check. When fixed: reference with bare #209 in the lane branch commits/PR (pipeline closes on publish); if investigation proves it already fixed on branch, close #209 immediately with evidence comment.
