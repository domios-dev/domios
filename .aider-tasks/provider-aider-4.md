# Make Aider work live in Domios DEV, including Chat

The test command now ends with a LIVE smoke in the owner's DEV instance (C:/Users/User/.local/bin/aider-provider-smoke.sh, read-only reference): it validates and installs this plugin, runs `agent spawn aider --model openai/mimo-v2.6-pro`, checks the tab is recognized (agent_type aider) and ready (boot_stage agent_ready), sends a message through the normal Chat send path asking for an exact token, and requires the assistant reply to appear in the tab's Chat (`codeterm tab chat`), then closes the tab. Make the whole test command pass.

Likely work (verify against the references, do not guess API names):
1. Launch/readiness must really work for Aider 0.86 in a PTY: correct command line, readiness when the `>` input prompt is shown, idle/busy states.
2. Chat transcript: Aider writes the conversation to `.aider.chat.history.md` in its working directory (user turns start with `#### `, assistant text follows). Implement the same transcript/structured-chat capability that plugins/provider-opencode implements (see its plugin.ts and sessionContent.ts) so the Chat UI shows user and assistant messages for this tab, reading only the history file of this tab's cwd and only content appended after this tab's launch.
3. Keep unit tests green and extend them for the history parser (user turn, assistant reply, multi-turn, content before launch excluded).
Commit per fix, English messages. If the smoke prints SMOKE FAIL, read its message: it says exactly which stage failed.
