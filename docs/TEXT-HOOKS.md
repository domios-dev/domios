# Inbound message hooks (text_hooks)

A user-run daemon can intercept inbound bridge messages (Telegram, Discord, etc.) **before** they reach the General Agent and decide what to do.

## Three intervention modes

| Mode | Effect on GA |
|------|--------------|
| **forward (unchanged)** | GA sees the original text. Default behaviour. |
| **replace** | The daemon already answered the user — **GA does not see the message at all**. Useful for fast-path commands the daemon handles itself (e.g. lights on/off via Home Assistant). |
| **forward_with_metadata** | GA sees the text PLUS a `[Hook metadata: ...]` line at the top — the daemon classified it; GA should **respect the metadata, not re-classify**. |

## The `[Hook metadata: ...]` rule for GA

When you see a message that starts with `[Hook metadata: ...]`, the daemon has already done classification work. **Trust it, don't redo it.** Common metadata fields:

- `router_label: <slug>` — the daemon classified the request type (e.g. `light_cmd`, `weather`, `note_capture`). Act on this label, don't run your own classifier.
- `confidence: 0.8` — daemon's confidence in the classification.
- `chained: true` — message is part of a multi-turn flow the daemon is tracking.

If a metadata field tells you a classification you wouldn't have made yourself — **trust the daemon**. The user set it up because it knows something you don't (cheaper model, structured data, local context).

## Config

Under `[integrations.text_hooks]` in `~/.codeterm/config.toml`:

```toml
[integrations.text_hooks]
enabled = true
daemon_url = "http://127.0.0.1:7892"
timeout_ms = 2000
fallback_on_error = "forward"   # forward | replace | reject — what to do if daemon doesn't respond in time
```

The bridge POSTs each inbound message to `daemon_url` as JSON:
```json
{
  "service": "telegram",
  "user_id": 123456789,
  "text": "what's the weather?",
  "metadata": { "msg_id": 42 }
}
```

The daemon returns:
```json
{
  "mode": "forward_with_metadata",
  "text": "what's the weather?",
  "metadata": { "router_label": "weather", "confidence": 0.92 }
}
```

If `mode = "replace"`, the daemon may also include `"reply"` — the bridge will send that back to the user as if the GA had responded.

## Why users set this up

Common patterns:

- **Cheap classifier in front of GA.** A small regex/Groq router handles trivial commands (lights, weather, smart-home macros) in <100ms without burning Claude tokens.
- **Structured intent capture.** Daemon parses "buy 2 kilos of tomatoes" into a structured shopping-list entry and replies "added"; GA never sees it.
- **Routing across multiple GAs.** Daemon picks which machine's GA should handle a request based on context (e.g. "@blackstar deploy" → routes to BLACKSTAR's GA).

You don't usually configure text_hooks yourself as the agent — that's the user setting up infrastructure. Your job is to **honor the metadata** when it arrives.

## Debugging

If the user reports "my classifier isn't working":
1. Check `[integrations.text_hooks] enabled = true` in config.
2. `curl -X POST http://127.0.0.1:7892` (or whatever `daemon_url` is) — daemon process up?
3. Tail `~/.codeterm/logs/codeterm.log` — look for `[bridge] text_hook ...` lines showing what the daemon returned.
4. If daemon returns `{}` or invalid JSON, the bridge falls back to `fallback_on_error` behaviour.

<!-- revision: 1 -->
