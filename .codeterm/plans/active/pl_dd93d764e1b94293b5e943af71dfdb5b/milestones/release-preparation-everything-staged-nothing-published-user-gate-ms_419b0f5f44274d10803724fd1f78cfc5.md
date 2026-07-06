---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_419b0f5f44274d10803724fd1f78cfc5
handle: release-preparation-everything-staged-nothing-published-user-gate-ms_419b0f5f44274d10803724fd1f78cfc5
title: evidence
---

Release prep artifacts at release/v1.6.2@a23bc5fe. PR #211 open to master and mergeable. Checks: npm run check:versions PASS; cargo check cli PASS; cargo check src-tauri --lib PASS. Local build produced src-tauri/target/release/bundle/macos/CodeTerm.app, src-tauri/target/release/bundle/dmg/CodeTerm_1.6.2_aarch64.dmg sha256=3a280f84b0af1f7b7b2402a36606254287d2b6a2b874cb236ea58f556d3c53e6, src-tauri/target/release/bundle/macos/CodeTerm.app.tar.gz sha256=8d7601fef32a298009ac97ee8ddf927fcb452822e6e9f8f1573b62c605979594. codesign --verify and spctl accepted the app as Notarized Developer ID. Caveats: updater .sig absent; tauri build wrapper hung after bundle list and was stopped. Runner readiness: ROG-windows-build online, ubunta-linux-build online, codeterm-builder offline/no local runner install found.
