# Telegram Bridge

Connect a Telegram bot to the General Agent. CodeTerm handles inbound routing; the agent sends replies via CLI.

## Setup

1. Create a bot via @BotFather, get the token
2. Save token as plain text to `~/.codeterm/secrets/telegram.token` (ONLY the token, no markdown):
   ```bash
   mkdir -p ~/.codeterm/secrets
   echo -n "YOUR_BOT_TOKEN_HERE" > ~/.codeterm/secrets/telegram.token
   ```
3. Enable and restrict users (optional):
   ```bash
   codeterm config set integrations.telegram_enabled true
   codeterm config set integrations.telegram_allowed_users '["123456789"]'
   ```
4. Restart CodeTerm (hot-reload not yet supported)

## How it works

- **Inbound** (Telegram → Agent): messages arrive as `[Telegram: user_id] text` in the GA tab. Messages within 300ms are debounced into one delivery. Typing indicator is sent automatically.
- **Outbound** (Agent → Telegram): `codeterm bridge send` from any tab, not just GA. With BotFather topics enabled, a tab's sends and notifications land in that tab's own topic with a `title · role · workspace · machine` header and a linked pointer in the main chat; the General Agent stays in the main chat.
- **Threads**: a message in a tab's topic, or a reply to a tab's message, reaches that tab (marker carries `thread=`). Closing the tab deletes its topic.
- UTF-8 safe: chunks Cyrillic/emoji/multi-byte correctly.
- Resilient: auto-restarts polling after 5 seconds on failure.

## Sending text

Text is positional, not a flag:

```bash
codeterm bridge send --to USER_ID --service telegram "Your reply"
```

`--append` concatenates to the previous message with an automatic `\n\n` break, edits in-place within 60s:

```bash
codeterm bridge send --to USER_ID --service telegram --append "...more text"
```

Use append for streaming: first `send`, then repeated `--append`.

## Sending media

```bash
codeterm bridge send --to USER_ID --type photo --file /path/to/img.png --caption "Check this"
codeterm bridge send --to USER_ID --type document --file /path/to/report.pdf --filename "Q1-report.pdf"
codeterm bridge send --to USER_ID --type voice --file /path/to/audio.ogg
codeterm bridge send --to USER_ID --type video --file /path/to/video.mp4
codeterm bridge send --to USER_ID --type animation --file /path/to/anim.gif
codeterm bridge send --to USER_ID --type sticker --file /path/to/sticker.webp
```

Supported `--type`: `text` (default), `photo`, `document`, `voice`, `animation`, `video`, `sticker`. Positional text becomes the caption if `--caption` is not set. Stickers have no caption.

## Per-tab topic threads

Open or reuse your own topic and print its link:

```bash
codeterm bridge thread open --service telegram
```

Only the General Agent may open another tab's topic, then reply in the main chat with the printed link:

```bash
codeterm bridge thread open --tab TAB_ID --service telegram
```

Any other tab naming a foreign `--tab` is refused and nothing is created. Without BotFather topics the owner is told once and sends stay in the main chat. Full contract: `codeterm docs TELEGRAM-SETUP`.

## Formatting

HTML only: `<b>`, `<i>`, `<code>`, `<pre>`, `<a href>`, `<s>`, `<u>`, `<tg-spoiler>`.

## Settings UI

Settings → Integrations → Telegram (toggle + allowed users).

## Hot-relocating the poller (`bridge migrate`)

Telegram allows exactly one active long-poll per bot token. `bridge migrate` moves that poller from the current machine to a paired mesh peer without restarting either daemon. Use it when you're shutting down the host that currently runs the bridge.

**Run on the SOURCE machine.** `--mesh PEER bridge migrate` exits `6` (usage error) — the orchestration legitimately needs to talk to both daemons in opposite directions.

```bash
codeterm bridge migrate --to PEER \
  [--service telegram] \
  [--announce] [--announce-chat-id 186809246] \
  [--no-rollback] [--force] [--claim-timeout-ms 10000]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--to` | *(required)* | Target peer `instance_name` or `instance_id` from `codeterm mesh peers`. |
| `--service` | `telegram` | Bridge to migrate. Only `telegram` today. |
| `--announce` | `false` | Post `[CodeTerm] Telegram bridge migrated to <to> from <from>` after success. |
| `--announce-chat-id` | `186809246` | Chat id for `--announce`. |
| `--no-rollback` | `false` | If the target claim fails, leave the source disabled instead of restarting it. |
| `--force` | `false` | Skip the target preflight (`token_present || has_secret_in_vault`). |
| `--claim-timeout-ms` | `10000` | How long the target may take to claim the upstream lock before the migration is failed. |

**What it does (briefly):** disable source config → stop source supervisor → enable target config → start target supervisor with `ensure_token: true` (target pulls from its own vault if needed) → wait for target's `last_poll_ok >= started_at` → verify both states → optionally announce. On failure, auto-rollback re-enables and restarts the source.

**Secret locality.** Each daemon resolves its own vault. The CLI never reads `telegram_bot_token` from the source and never sends it across the wire — `--ensure-token` only fires on the daemon that owns the secret. Same contract as `--mesh PEER mem secret …`.

**Exit codes:** `0` success / `1` generic failure (rollback OK) / `5` preflight (target unreachable, no token + no vault, missing peer) / `6` usage error (e.g. `--mesh`) / `7` split-brain (rollback also failed; CLI prints the manual repair command). Full orchestration steps + REST endpoints: `docs/MESH.md` and `docs/API.md`.
