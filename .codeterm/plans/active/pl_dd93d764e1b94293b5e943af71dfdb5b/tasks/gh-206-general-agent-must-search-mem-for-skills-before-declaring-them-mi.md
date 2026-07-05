---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_9de244fa7db544d5aab939c0da7f4e76
handle: gh-206-general-agent-must-search-mem-for-skills-before-declaring-them-mi
title: 'GH #206: General Agent must search mem for skills before declaring them missing'
status: done
---

STEP 1: gh issue view 206 — issue body is the spec. The General Agent prompt declares skills missing without first searching mem. Fix in src-tauri/prompts/general_agent.md (and any sibling templates): add an explicit directive — before answering 'I don't have a skill for X', run codeterm mem search for the skill (search strategy: keyword then semantic reformulation), and only then declare absence. Keep it terse and English-only, no personal references (prompt hygiene rule). Add/extend the prompt-wiring test (prompts.rs tests assert key phrases) so the directive can't silently vanish. This is a small, well-bounded lane.
