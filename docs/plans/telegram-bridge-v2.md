# Telegram Bridge v2 — Streaming Architecture

## Current (v1): Request-Response
```
Telegram msg → BridgeManager → write to PTY → wait Working→Idle → capture output → send back
```
Problem: one message at a time, no streaming, agent doesn't see who's talking.

## Target (v2): Streaming Bridge

### Core idea
CodeTerm is the middleman — it reads PTY output continuously and streams new agent messages to Telegram. Incoming Telegram messages are injected into PTY with a source prefix.

### Message format
- **From Telegram user:** `[Telegram: 186809246] message text\r`
- **From another agent:** `[Agent: blackstar-general] message text\r`
- **Agent output → Telegram:** CodeTerm reads new output from OutputBuffer, strips ANSI, chunks, sends to Telegram chat

### Architecture
```
Telegram ←→ CodeTerm Bridge ←→ General Agent PTY
                                      ↑
                                OutputBuffer (already populated by frontend read loop)
```

### How it works

1. **Outbound (Agent → Telegram):**
   - Bridge subscribes to OutputBuffer changes for the General Agent PTY
   - On new output: diff since last sent offset
   - Parse: strip ANSI, detect new "assistant" messages (not user echo)
   - Chunk for Telegram (4096 chars) and send
   - Need to distinguish agent output from user input echo — use agent status tracking (Working→Idle transitions) or parse structured output

2. **Inbound (Telegram → Agent):**
   - Telegram message arrives via long poll
   - CodeTerm writes to PTY: `[Telegram: {user_id}] {text}\r`
   - Agent sees it as a new message with source attribution
   - Agent responds normally — output gets streamed back

3. **Agent configures bot:**
   - Agent creates `~/.codeterm/secrets/telegram.token` with the bot token
   - Agent calls `codeterm config set integrations.telegram_enabled true`
   - CodeTerm picks up config change and starts bridge

### Pane icon
- When Telegram is connected to a pane, show a muted Telegram icon after the pane title
- Store `connected_services: string[]` in Pane type (e.g. ["telegram"])
- Frontend renders icon based on this field

### Multi-agent messaging (future)
- Agents on different machines can message each other
- Format: `[Agent: {machine}-{agent_id}] message`
- Transport: CodeTerm REST API between machines (already have remote API)
- One General Agent is "primary" (connected to Telegram)
- Others are "secondary" — can be promoted if primary goes down

### Distributed failover (future)
- General Agents register with each other via REST API
- Heartbeat between machines
- If primary stops responding, secondary takes over Telegram connection
- Primary instructions can be synced: `codeterm config` or shared EXTRA_INSTRUCTIONS.md

### Implementation order
1. Streaming outbound (Agent → Telegram) — watch OutputBuffer, send new content
2. Prefixed inbound (Telegram → Agent) — `[Telegram: id] text`
3. Pane icon for connected services
4. Agent self-setup (create token, enable bridge)
5. Multi-agent messaging
6. Distributed failover
