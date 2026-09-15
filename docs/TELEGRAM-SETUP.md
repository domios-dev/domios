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

   The allow-list now accepts either quoted strings (`["123456789"]`) or bare integers (`[123456789]`) — the deserializer coerces. Older builds (pre v1.2.1) only accepted quoted strings and silently rejected bare ints; if you're on an older build and inbound messages log `[telegram] rejected user <id>` despite the id matching, quote them.

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

## Escaped newlines from the CLI

`codeterm bridge send` decodes each literal `\n` in its text argument into a
Telegram newline. This keeps quoted one-line shell commands readable:

```bash
codeterm bridge send --to 123456789 --service telegram "first line\nsecond line"
```

Use `--raw-text` only when the recipient must receive the literal characters
backslash and `n` instead of a line break. Tabs are unchanged; actual tabs pass
through as supplied by the shell.

## Photos

```bash
codeterm bridge send --to USER_ID --type photo --file /path/to/image.jpg
```

CodeTerm has no screenshot CLI. If the user asks for a screen capture, ask them to paste an image or provide a file path.

## Per-tab topic threads

With topics enabled for the bot in BotFather, each tab talks to the owner in its own topic of the owner's private chat:

- The first notification or `bridge send` from a tab creates one topic named exactly after the tab title. Only while another live tab shares that title does it get a short `#xxxx` suffix from the tab id. Later sends reuse the binding, stored in `~/.codeterm/bridge/telegram-threads.json` (ids and topic names only).
- Every tab post and its main-chat pointer start with the sender header `title · role · workspace · machine`, where role is `orchestrator`, `worker`, `planner` or `general`. The pointer links into the topic; `--choice` buttons ride the topic post.
- The main chat (no thread, or the general topic) stays the General Agent conversation, and the General Agent's own sends stay there with its header.
- A message in a bound topic reaches that tab with `thread=<id>` in its Telegram marker. A reply to a tab's message reaches that tab even without topics. Everything else reaches the General Agent; `origin=closed_thread|closed_tab|unbound_thread` says when it came from a retired or unknown thread.
- With topics off, sends fall back to the main chat with the header and the owner is told once to enable topics in BotFather. Creation is not retried per message; a bridge restart probes again.
- Closing a tab deletes its topic (history loss is intended) and removes the binding, so a later tab with the same title gets a fresh topic. If deletion fails, the topic gets a closing note and a `[closed]` name, and later messages there reach the General Agent.

Open or reuse a topic and print its link:

```bash
codeterm bridge thread open [--tab ID] [--service telegram]
```

Without `--tab` it opens the caller's own topic. Only the General Agent may name another tab: the topic belongs to the target, carries its header, and routes replies to it. Any other caller naming a foreign tab is refused (socket `bridge.thread.open`, code `-32003`) before any Bot API call. The first open posts `you can talk to <tab> here`; repeated opens create nothing. The tab menu's **Connect Telegram** and **Disconnect Telegram** run the same operations; **Connect Telegram** is also in the sidebar tab menu.

## Cross-machine bridge migration

To move the active poller to a paired peer without downtime:
```bash
codeterm bridge migrate --to PEER [--announce] [--no-rollback] [--force] [--claim-timeout-ms 10000]
```

Run on the **source** machine. `--mesh PEER` is rejected (exit 6) because each daemon resolves its own vault and secrets never cross the mesh.

<!-- revision: 5 -->
