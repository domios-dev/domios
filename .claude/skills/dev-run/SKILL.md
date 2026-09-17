---
name: dev-run
description: Start, restart, kill, and verify the CodeTerm dev instance from inside this repo. Use whenever the user says "start dev", "restart dev", "dev up", "kill dev", "rebuild dev", asks to open/reload the app after code changes, reports the dev window is broken/not launching, or you need to verify that the running binary matches the source after an edit to src-tauri/ or src/. Covers build.sh commands, port 7686 health checks, log locations, clean-config resets, and platform-specific quirks.
---

# dev-run

This is a pointer, not a copy. The skill itself lives at `.agents/skills/dev-run/SKILL.md`; read that file and follow it.

Everything the skill needs — scripts, helpers, references — lives beside it under `.agents/skills/dev-run/`. Never edit this pointer to carry procedure, and never copy the skill here: one source, no drift.
