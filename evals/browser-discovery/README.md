# browser-discovery

Checks that a freshly spawned agent finds the built-in browser on its own. Each case
spawns an agent with a plain web task and no mention of Domios, then passes only when
the agent opens its own browser tab (`codeterm plugin browser ...`) and reaches the
target page. Reaching for a provider extension (Claude in Chrome, computer use) fails.

The signal comes from the core plugin prompt line (`agentPromptLine` in
`plugins/browser/plugin.json`) and `codeterm docs BROWSER`. Rerun after any change
to agent prompts, the browser manifest or its guide.

```bash
# default cases: claude:haiku and codex
CODETERM_INSTANCE=surfaces CODETERM_API_PORT=7690 bash evals/browser-discovery/run.sh
# pick cases as provider[:model]
bash evals/browser-discovery/run.sh claude:haiku opencode
```

The exit code is the number of failed cases. Overrides:
- `CODETERM_BROWSER_DISCOVERY_TASK` changes the task.
- `CODETERM_BROWSER_DISCOVERY_EXPECT` changes the URL fragment the agent must reach.
- `CODETERM_BROWSER_DISCOVERY_TIMEOUT` changes the per-case timeout in seconds; the default is 240.

Pages with consent banners (e.g. Google in the EU) are a poor default: some agents
correctly stop and ask the user, which is right but not what this bench measures.
