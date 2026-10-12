# Blueprint watch verification

`evals.json` holds behaviour scenarios for agents that create Blueprint watches. A created graph is only admitted. The agent must show one real probe execution and a recognized bucket in `codeterm blueprint show` before reporting that the watch works.

Run it with the prompt-evaluator skill (`.agents/skills/prompt-evaluator`) against the agent primer, `src-tauri/prompts/agent_codeterm_primer.md`, and `prod-docs/BLUEPRINT-RECIPES.md`. A run passes when the answer for the unclassified CRLF watch repairs the pattern with `(?m)` line anchors instead of declaring success. For the empty Runs view, a passing answer verifies the probe watch through `blueprint show` and does not report failure or a UI bug. No scenario starts a DEV instance or creates a live graph.
