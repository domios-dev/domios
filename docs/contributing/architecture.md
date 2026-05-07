# Architecture: agent providers, multi-agent design, detection

## Agent providers

Agent detection and integration is abstracted into providers. Currently only Claude Code is implemented, but the architecture supports any CLI agent.

**What a provider does:**
- **Detect** — recognize when an agent starts in a PTY (OSC title, output patterns, process name)
- **Session** — link to session files for structured data (Claude: JSONL, Codex: TBD)
- **Parse** — extract structured messages from agent output (chat view)
- **Control** — send input to the agent, understand its prompt format

**Current providers:**
- `claude` — detects via OSC title containing "claude", reads `~/.claude/projects/` JSONL, parses messages/tool_calls/thinking blocks

**Future providers (architecture ready, not implemented):**
- `codex` — OpenAI Codex CLI
- `aider` — Aider pair programmer
- `goose` — Block's Goose agent
- `generic` — fallback, terminal output parsing only

Provider code lives in:
- Rust: `src-tauri/src/agent/` — detection, session file reading
- TypeScript: `src/components/ChatView/parser.ts` — output parsing
- TypeScript: `src/components/Terminal/TerminalPane.tsx` — detection hooks (onTitleChange)

## Multi-agent mindset

**Always design for multiple agent types, not just Claude.** Every feature that interacts with agents must go through the AgentProvider abstraction. Never hardcode Claude-specific logic in generic code. If a behavior is Claude-specific (clipboard paste for images, JSONL session format, TUI noise patterns), it belongs in ClaudeProvider or the Claude provider config — not in ChatView, TerminalPane, or other generic components.

When adding agent-related features, ask: "How would this work with Codex? With Aider? With a local model?" If the answer requires Claude-specific code, put it in the provider.

## Detection: structural patterns, not literal strings

Detection code (prompts, task completion, agent identification, output parsing) must use structural fingerprints, not literal text. If Claude renames "Do you want to proceed?" to "Do you want to create?", detection should still work — match the structure (numbered options + footer), not the wording. Vendors change copy without warning; structure is stable. Generalize, don't enumerate.

<!-- revision: 1 -->
