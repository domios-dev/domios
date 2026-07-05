---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_3ee49ba814ec45999a38befc20d10c81
handle: gh-209-linux-updater-fails-invalid-updater-binary-format-release-blocker-ms_3ee49ba814ec45999a38befc20d10c81
title: acceptance
---

Closed for feat/canvas-updates: diff review confirms deb/RPM manifest routing is already present on target branch via 7af748f8. release-linux.yml, release-manifest-validate.yml, docs, updater_routes.rs, update/channel.rs match fix/linux-updater-deb-auto for the actual fix. update.rs/apply.rs differences are older context/comment only; Cargo.lock version drift belongs to old master hotfix branch. Live updater round-trip remains a release-stage acceptance after latest-deb.json is published, not an open canvas implementation lane.