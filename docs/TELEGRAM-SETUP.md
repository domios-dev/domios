# Setting up a Telegram bot from scratch

When the user hands you a fresh Telegram bot token to wire up:

1. **Stash the token** (mode 600, gitignored path):
   ```bash
   printf '%s\n' '<token>' > ~/.codeterm/secrets/telegram.token && chmod 600 ~/.codeterm/secrets/telegram.token
   ```

2. **Allow the user**. In `~/.codeterm/config.toml` under `[integrations]`:
   ```toml
   telegram_enabled = true
   telegram_allowed_users = ["<user_id_as_quoted_string>"]
   ```

   The allow-list now accepts either quoted strings (`["186809246"]`) or bare integers (`[186809246]`) — the deserializer coerces. Older builds (pre v1.2.1) only accepted quoted strings and silently rejected bare ints; if you're on an older build and inbound messages log `[telegram] rejected user <id>` despite the id matching, quote them.

3. **Restart the bridge** so the new token / allow-list takes effect:
   ```bash
   codeterm bridge restart
   ```

4. **Smoke-check the token** without involving CodeTerm:
   ```bash
   curl https://api.telegram.org/bot<TOKEN>/getMe
   ```
   Returns `is_bot:true` plus the bot's username on success. If `401 Unauthorized`, the token is wrong; if `404`, the bot was deleted by BotFather.

5. **Have the user send a test message.** Tail the log to confirm receipt:
   ```bash
   tail -f ~/.codeterm/logs/codeterm.log
   ```
   Look for `[bridge] inbound from <name>`. If you see `[telegram] rejected user <id>`, the allow-list doesn't include them — go back to step 2.

## Common transient errors

- **409 Conflict** during restart (~1 second window): the previous `getUpdates` poller hasn't released. Normal.
- **Persistent 409**: a second daemon is polling the same bot (e.g. dev port 7686 AND prod port 7685 both have telegram_enabled), OR a webhook URL is set on the bot:
  ```bash
  curl https://api.telegram.org/bot<TOKEN>/getWebhookInfo
  curl https://api.telegram.org/bot<TOKEN>/deleteWebhook
  ```

## Telegram HTML formatting

The bridge sends with `parse_mode=HTML`. Allowed tags (Telegram Bot API whitelist):
- `<b>`/`<strong>`, `<i>`/`<em>`, `<u>`/`<ins>`, `<s>`/`<strike>`/`<del>`
- `<code>`, `<pre>`, `<pre><code class="language-XXX">...</code></pre>` (syntax-highlighted on Premium clients)
- `<a href="...">` (also `tg://user?id=N`)
- `<tg-spoiler>`, `<span class="tg-spoiler">`
- `<blockquote>` (with optional `expandable` attribute)

**Markdown is NOT supported.**

### Pitfalls

Any OTHER `<word>` substring fails parsing — Telegram returns 400 and rejects the whole message. Common offenders:

- Placeholders like `<value>`, `<date>`, `<id>` — escape or rewrite as `[value]`.
- Generics like `Vec<T>`, `HashMap<K, V>` in code — escape inside `<code>`: `Vec&lt;T&gt;`.
- HTML examples that mention tag names — escape: "the `&lt;b&gt;` tag means bold".

Inside `<code>` and `<pre>` blocks, escape `<` `>` `&` as `&lt;` `&gt;` `&amp;` — code blocks are NOT a safe haven for raw angle brackets, since Telegram still parses HTML inside them.

The bridge has a sanitizer (`src-tauri/src/bridge/sanitize.rs`) that auto-fixes most of this transparently: stray `<word>` substrings get escaped, unclosed tags auto-closed, unsafe attributes stripped. But write clean HTML — the sanitizer is a safety net, not a band-aid.

## Long replies & continuation

Send the first chunk normally, then use `--append` for follow-up text. The bridge edits the previous message in place (within 60s) by concatenating new text:

```bash
codeterm bridge send --to 123456789 --service telegram "Here's what I found:"
codeterm bridge send --to 123456789 --service telegram --append "1. Item one
2. Item two"
```

The user sees both parts merged into one Telegram message. **Send only the new text** to `--append`, not the full message — concatenation is server-side.

## Photos

```bash
codeterm bridge send --to USER_ID --type photo --file /path/to/image.jpg
```

CodeTerm has no screenshot CLI. If the user asks for a screen capture, ask them to paste an image or provide a file path.

## Cross-machine bridge migration

To move the active poller to a paired peer without downtime:
```bash
codeterm bridge migrate --to PEER [--announce] [--no-rollback] [--force] [--claim-timeout-ms 10000]
```

Run on the **source** machine. `--mesh PEER` is rejected (exit 6) because each daemon resolves its own vault and secrets never cross the mesh.

<!-- revision: 1 -->
