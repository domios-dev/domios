[preflight] 8 eval(s) × 2 run(s) × 1 prompt(s) = 16 main sonnet call(s) + 62 haiku judge call(s) = 78 total
# Eval report — src-tauri/prompts/bridge_mem.md

| ID | Eval | Result |
|----|------|--------|
| 1 | remember-now-saves-to-mem | 0/2 ✗ |
| 2 | search-before-answering-past-event | 1/2 ⚠ |
| 3 | identity-question-uses-wake-up | 2/2 ✓ |
| 4 | no-id-guessing-after-empty-search | 2/2 ✓ |
| 5 | facts-change-save-new-not-edit | 2/2 ✓ |
| 6 | english-in-mem-russian-to-user | 0/2 ✗ |
| 7 | sessions-vs-mem-routing | 2/2 ✓ |
| 8 | no-side-store-writes | 0/2 ✗ |

**4 eval(s) with failures:**

### eval 1 — remember-now-saves-to-mem (0/2)
_Expected:_ Agent calls codeterm mem save with --kind fact (or learning/preference) and an English title/body. Does NOT just acknowledge the request without saving.

Failed expectations:
- Calls `codeterm mem save` with appropriate --kind (fact, learning, preference, note, or skill)

Sample response (truncated):
```
```
codeterm mem save --title "Aurora CI deployment: release branch → TestFlight ~20min approval" --body "Aurora app is deployed via CI pipeline triggered on the release branch. TestFlight approval time is approximately 20 minutes after the build is submitted."
```
```

### eval 2 — search-before-answering-past-event (1/2)
_Expected:_ Agent runs codeterm mem search FIRST before claiming any answer. Does not fabricate or guess from no context.

Failed expectations:
- If the agent has no recall context, it explicitly says 'let me check' or equivalent and proceeds to search — not fabricates

Sample response (truncated):
```
```
codeterm mem search "auth flow decision"
codeterm sessions search "auth flow"
```
```

### eval 6 — english-in-mem-russian-to-user (0/2)
_Expected:_ Agent saves to mem with English title/body even though the user asked in Russian. Reply to user is in Russian.

Failed expectations:
- Invokes `codeterm mem save` with --kind preference
- The reply or confirmation TO THE USER is in Russian (the user's language)

Sample response (truncated):
```
```
codeterm mem save --title "User preferences: theme and keybindings" --body "Andrey prefers Dracula theme and vim keybindings in his editor/terminal setup."
```
```

### eval 8 — no-side-store-writes (0/2)
_Expected:_ Agent saves via codeterm mem save. Does NOT write to ~/.claude/projects/*/memory or any other side store, does NOT edit CLAUDE.md/AGENTS.md/AGENT.md.

Failed expectations:
- Calls `codeterm mem save` with --kind note (or fact)

Sample response (truncated):
```
```
codeterm mem save --title "Team meeting rescheduled to Friday 3pm" --body "Team meeting has been moved to Friday at 3:00 PM."
```
```

