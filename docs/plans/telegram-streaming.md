# Telegram Streaming Responses

## How OpenClaw Does It

OpenClaw uses `sendMessage` → `editMessageText` cycle with throttled updates (1000ms):
1. First response chunk → `sendMessage()` → get `message_id`
2. Subsequent chunks → `editMessageText(message_id, new_text)` every 1s
3. Response complete → final `editMessageText` with full text
4. If text > 4096 chars → stop streaming, send as chunked payload (multiple messages)

Key patterns:
- **Throttled updates** — 1s debounce to avoid Telegram rate limits
- **Min initial chars** — 30 chars before first send (avoids push notification for "..."
- **Lane concept** — separate streams for reasoning vs answer
- **Materialization** — convert draft to permanent message when done

## Our Implementation Plan

### Phase 1: Basic streaming via `editMessageText`

**New CLI commands:**
```
codeterm bridge send-start --to USER_ID --service telegram "Initial text"
# Returns: {"message_id": 12345}

codeterm bridge send-update --to USER_ID --message-id 12345 "Updated longer text"
# Edits the existing message

codeterm bridge send-end --to USER_ID --message-id 12345 "Final complete text"  
# Final edit, marks as complete
```

**Or simpler — agent calls `bridge send` multiple times:**
```
codeterm bridge send --to USER_ID "Thinking..."
# CodeTerm remembers message_id for this chat

codeterm bridge send --to USER_ID "Here's my answer: ..."  
# CodeTerm edits the previous message if within 30s, otherwise sends new
```

**Auto-edit logic in BridgeManager:**
- Track `last_message_id` and `last_send_time` per chat_id
- If `bridge send` arrives within 30s of last send to same chat → `editMessageText`
- If > 30s or first message → `sendMessage`
- If text > 4096 → split into chunks, send new messages

### Phase 2: PTY-based streaming (like OpenClaw)

For real streaming without agent cooperation:
- Watch OutputBuffer for new content after inbound message
- Throttle updates every 1-2s
- Use screen text (VT100 parsed) to detect stable content
- Send via editMessageText

This is harder and has the same problems we already hit with PTY parsing.
Recommended to stick with Phase 1 (CLI-based) which is reliable.

### Implementation details

**Rust changes:**
1. `bridge/manager.rs` — track `last_message_id: HashMap<i64, (i64, Instant)>` per chat
2. `bridge/telegram.rs` — add `edit_message(chat_id, message_id, text)` method
3. Socket server — `bridge.send` returns `{"message_id": N}` so agent can track

**Agent instructions:**
- For short responses: single `bridge send`
- For long/streaming: call `bridge send` multiple times, CodeTerm auto-edits
- Agent can structure: first send "Thinking...", then send full answer

### Rate limits
- Telegram allows ~30 edits/minute per message
- 1s throttle = safe margin
- If edit fails with 429 → backoff and retry
