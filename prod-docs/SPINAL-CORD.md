# Spinal Cord (Supporter Agent)

Spinal Cord is Domios's small decision assistant: it helps your agents find useful answers and notices when unattended work needs attention. The CLI calls it `supporter`; `spinal-cord` is an alias of the same command.

## What it does for you

- **Recall that puts answers first.** It reranks memory by how directly it answers your question, rather than mere topic similarity. Recall can include linked knowledge and embedded help, so an agent can find the context it needs without making you repeat it. It ranks evidence; your agent writes the answer.
- **Noticing stuck tabs.** It checks tab events and recent screen context for waiting prompts, apparent lack of progress and errors. Domios can surface blockers and answer eligible provider-declared trust, tool-permission and update choices. Login, authentication and quota problems need your attention. Model-assisted recovery requires qualified judgments and independent process evidence; it does not restart a live agent just because its screen looks stuck.
- **Quiet Team heartbeats.** It judges whether a changed Team observation needs action before Domios wakes the Orchestrator with a next step. Unchanged observations are not sent twice; elapsed silence alone never wakes the Orchestrator. Without a connected adapter that supports choices, or on denial, uncertainty, failure or an exhausted cap, no automatic reminder is sent. Reports and owner messages continue through their own channels.

Context routing between agents by Spinal Cord is not available yet; incoming-work triage is a future direction. Domios's existing routing delivers messages and reports independently.

It is worth connecting when you have several agents or unattended tasks: better recall and visible blockers reduce the context you must repeat and the tabs you must watch. If the model fails or runs out of credits, ordinary recall and deterministic tab handling remain available.

## Connect it

You need a registered adapter and a model it lists. A remote adapter may require an API token; the adapter declares whether a token is required, optional or unnecessary. Do not assume a model name or ask the user to paste a secret into chat.

First discover the available adapters, models and token requirements:

```text
codeterm supporter models
```

For an adapter needing a token, store it **before activation**, using stdin only. Replace `<adapter>` with the discovered id. On Windows, copy the token to the clipboard, then run in PowerShell:

```powershell
Get-Clipboard -Raw | codeterm supporter token set --adapter <adapter>
```

On macOS, use `pbpaste | codeterm supporter token set --adapter <adapter>`; on Linux, pipe the output of your secret manager to that same command. The token value must never appear in argv, shell history, memory, logs or agent messages. Clear the clipboard afterward if you used it.

Select and activate the adapter, optionally specifying its listed model:

```text
codeterm supporter use <adapter> --model <model>
codeterm supporter status
codeterm supporter status --json
```

Omit `--model <model>` to use the adapter's saved or default model. `use` connects and activates immediately; there is no separate `enable` command or feature switch. If activation reports `saved_not_active`, resolve the reported issue and retry `use`.

Verify that human status shows Active with your adapter and model; JSON status must show `connected: true` and no unavailable, budget or credit error. Saved configuration or token presence alone does not prove a successful model call. After ordinary memory searches or tab activity, check status for activity and calls. Relevant memory and bounded recent tab screen text are sent to the chosen adapter; consider that when choosing a local or remote service.

The General Agent can discover the options, explain them briefly and offer to connect your choice. Once you accept, it can perform the setup; obtain any required token through the stdin flow above, then verify status and report the result. Never claim connection while status reports a failure.

## Turn it off

```text
codeterm supporter disconnect <adapter>
codeterm supporter status
```

Disconnecting the active adapter stops Spinal Cord and removes that saved connection and its token. Usage history remains. There is no separate `disable` command; reconnect with the setup above when needed. Switching adapters with `use` preserves the other saved connections.

<!-- revision: 2 -->
