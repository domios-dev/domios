---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_2180aaf02193498c87f13f1cc9934bd0
handle: release-preparation-everything-staged-nothing-published-user-gate-ms_2180aaf02193498c87f13f1cc9934bd0
title: evidence
---

RELEASE PREP STAGED (in_review) — verified by Rossini on takeover.

STAGED ARTIFACTS (release/v1.6.2 @ a23bc5fe):
| item | state | detail |
| release PR #211 | OPEN, MERGEABLE | release/v1.6.2 -> master, unmerged (HARD STOP) |
| version sync | GREEN | npm run check:versions PASS |
| fast prechecks | GREEN | cargo check cli PASS; cargo check src-tauri --lib PASS |
| local prod build | PRESENT on disk | CodeTerm.app + dmg + app.tar.gz under src-tauri/target/release/bundle |
| CodeTerm_1.6.2_aarch64.dmg | 66MB | sha256=3a280f84b0af1f7b7b2402a36606254287d2b6a2b874cb236ea58f556d3c53e6 (re-verified matches) |
| CodeTerm.app.tar.gz | 68MB | sha256=8d7601fef32a298009ac97ee8ddf927fcb452822e6e9f8f1573b62c605979594 |
| signing | GREEN | codesign --verify + spctl accept app as Notarized Developer ID |
| runners | ROG online, ubunta online | codeterm-builder (Mac) OFFLINE |

TWO ATTENTION ROWS (human/farm):
1. Mac runner 'codeterm-builder' OFFLINE — must come online before tag; release.yml (macOS native + arm64 Linux Docker) has no runner until then.
2. Updater .sig absent locally — FARM CONCERN, not a local gap: each runner signs at build time via TAURI_SIGNING_PRIVATE_KEY; verify .sig present in published assets after the farm build.

Runtime launch-verify is deliberately Andrey's step (this IS his test build); bundle validity confirmed via codesign+spctl. Zero tags pushed, master untouched.
