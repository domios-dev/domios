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

- **Inbound** (Telegram → Agent): messages arrive as `[Telegram: user_id] text` in the GA pane. Messages within 300ms are debounced into one delivery. Typing indicator is sent automatically.
- **Outbound** (Agent → Telegram): `codeterm bridge send` from any pane, not just GA.
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

## Formatting

HTML only: `<b>`, `<i>`, `<code>`, `<pre>`, `<a href>`, `<s>`, `<u>`, `<tg-spoiler>`.

## Settings UI

Settings → Integrations → Telegram (toggle + allowed users).
