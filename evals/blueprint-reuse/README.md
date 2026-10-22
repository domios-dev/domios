# Blueprint reuse

`evals.json` holds behaviour scenarios for agents about to author a Blueprint. Reuse comes first: a saved recipe with new inputs, then a built-in node such as the GitHub source, and only then a custom command source.

Run it with the prompt-evaluator skill (`.agents/skills/prompt-evaluator`) against the agent primer, `src-tauri/prompts/agent_codeterm_primer.md`, `src-tauri/prompts/general_agent.md`, `prod-docs/BLUEPRINTS.md` and `prod-docs/BLUEPRINT-RECIPES.md`. A run passes when the answer reuses a recipe with new inputs before authoring a graph, discovers the built-in GitHub node before writing a shell source, and still allows a custom command source when no recipe or built-in fits. Every scenario is text-only; none starts a DEV instance or creates a live graph.
