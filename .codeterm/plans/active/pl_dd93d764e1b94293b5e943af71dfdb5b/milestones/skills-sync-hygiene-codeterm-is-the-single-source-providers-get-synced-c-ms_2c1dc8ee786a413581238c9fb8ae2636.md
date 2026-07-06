---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_2c1dc8ee786a413581238c9fb8ae2636
handle: skills-sync-hygiene-codeterm-is-the-single-source-providers-get-synced-c-ms_2c1dc8ee786a413581238c9fb8ae2636
title: status_update
---

STATUS: in_review

CHANGES: src-tauri/src/commands/skills.rs (bf21ebe3) — added report_provider_orphans() + wired into mirror_skills_to_providers() + unit test. FS: deleted mac-dev-test-skill from ~/.claude/skills/ and ~/.agents/skills/.

DISPOSITION TABLE:
| skill              | canonical | .claude | .agents | mem   | action                       |
|--------------------|-----------|---------|---------|-------|------------------------------|
| atlassian          | ✓         | ✓       | ✓       | ✓     | in sync — OK                 |
| check-disk-space   | ✓         | ✓       | ✓       | ✓     | in sync — OK                 |
| codeterm           | ✓         | ✓ (identical) | ✓ (identical) | ✓ | in sync — OK          |
| codeterm-orchestrator | ✓      | ✓       | ✓       | ✓     | in sync — OK                 |
| file-share-nextcloud | ✓       | ✓       | ✓       | ✓     | in sync — OK                 |
| loom-transcript-fetch | ✓      | ✓       | ✓       | ✓     | in sync — OK                 |
| mac-dev-test-skill | ✗         | STRAY   | STRAY   | ✗     | empty scaffold, DELETED      |
| mem-testing        | ✓         | ✓       | ✓       | ✓     | in sync — OK                 |
| nextcloud-admin    | ✓         | ✓       | ✓       | ✓     | in sync — OK                 |
| product-bible-author | ✓       | ✓       | ✓       | ✓     | in sync — OK                 |

~/.claude/agents/{sub-worker,team-lead}.md: these are Claude Code Agent tool definitions (appear in Agent tool's available agent types). Intentional harness files, NOT skills. No action — keep as-is.

VERIFIED:
- All three dirs now match: confirmed via ls
- codeterm SKILL.md: canonical == .claude == .agents (diff showed identical, revision-based mirror works)
- Orphan guard: report_provider_orphans() runs after every mirror_skills_to_providers() call; logs [skills] orphan provider skill (no canonical source): <path>
- Unit test: orphan_detection_finds_provider_only_entries plants a fake stray, asserts only that entry is flagged
- NOT run: cargo check (orchestrator gate)

SYNC TRIGGER NOTE: mirror_skills_to_providers() fires at (a) startup and (b) agent_provider contribution-changed events. There is NO live file-watcher on the canonical skills dir. 'Edit canonical → copy updates' requires daemon restart or plugin reload, not a live push. This may be a gap worth a follow-up (watch ~/.codeterm/skills/ with notify and re-trigger mirror on change).

FOLLOWUP: GATES REQUESTED — cargo check src-tauri for orphan guard; Orchestrator should verify test passes with: cargo test -p codeterm commands::skills::mirror_tests::orphan_detection_finds_provider_only_entries
