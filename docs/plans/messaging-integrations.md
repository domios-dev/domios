# Messaging Integrations — Rich Content Architecture

## Research Summary

**OpenClaw patterns:**
- `ChannelOutboundAdapter` — cross-platform abstraction with `sendText`, `sendMedia`, `sendPayload`
- Inbound media: download files, create `<media:type>` placeholders for LLM
- Inline buttons: `InteractiveReply` → platform-native format (InlineKeyboard for Telegram)
- Reactions: `StatusReactionAdapter` with debounce/stall logic
- Media kinds: photo, document, audio, voice, video, video_note, animation, sticker

## Architecture

### Phase 1: Rich Outbound

**Unified OutboundMessage:**
```rust
#[serde(tag = "type")]
pub enum OutboundContent {
    Text { text: String },
    Photo { path: String, caption: Option<String> },
    Document { path: String, caption: Option<String>, filename: Option<String> },
    Voice { path: String, caption: Option<String> },
    Animation { path: String, caption: Option<String> },
    Video { path: String, caption: Option<String> },
    Sticker { path: String },
}
```

**CLI — single command with type:**
```bash
# Text (backward compatible)
codeterm bridge send --to ID "text"

# Photo
codeterm bridge send --to ID --type photo --file /path/to/img.png --caption "Look"

# Document
codeterm bridge send --to ID --type document --file /path/to/report.pdf

# Buttons
codeterm bridge send --to ID "Allow execution?" --buttons '[{"label":"Allow","data":"/approve x"}]'
```

### Phase 2: Rich Inbound

**Media download:**
1. Detect msg.photo/document/voice/etc in poll_updates
2. Download via getFile API → `~/.codeterm/bridge/media/{timestamp}_{file_id}.{ext}`
3. Inject: `[Telegram: ID] <photo:/path/to/file.jpg> caption text`

**Callback queries (button clicks):**
1. Poll captures callback_query
2. Answer immediately (dismiss spinner)
3. Inject: `[Telegram:callback: ID] /approve abc123 allow-once`

### Phase 3: Inline Buttons for Approval

Agent sends buttons → user taps in Telegram → callback routed to PTY.
Use for permission approval, confirmations, quick actions.

### Phase 4: Reactions

**Status reactions on inbound:**
- Message received → 👀 (acknowledging)
- Agent processing → 🤔 (thinking)
- Done → ✅
- Error → ❌

Debounce 700ms to avoid flicker.

### Phase 5: Cross-Platform Trait

```rust
#[async_trait]
pub trait MessagingService: Send + 'static {
    fn id(&self) -> &str;
    fn capabilities(&self) -> ServiceCapabilities;
    async fn start(self: Box<Self>, tx: Sender<InboundMessage>);
    async fn send_text(&self, chat_id: i64, text: &str, opts: SendOptions) -> Result<i64, String>;
    async fn send_photo(&self, chat_id: i64, path: &str, caption: Option<&str>, opts: SendOptions) -> Result<i64, String>;
    async fn send_document(&self, chat_id: i64, path: &str, caption: Option<&str>, opts: SendOptions) -> Result<i64, String>;
    async fn send_voice(&self, chat_id: i64, path: &str, opts: SendOptions) -> Result<i64, String>;
    async fn set_reaction(&self, chat_id: i64, message_id: i64, emoji: &str) -> Result<(), String>;
    async fn edit_text(&self, chat_id: i64, message_id: i64, text: &str) -> Result<(), String>;
}
```

**Platform capabilities:**
| Feature | Telegram | Discord | Slack |
|---------|----------|---------|-------|
| Photo | yes | attachment | file upload |
| Document | yes | attachment | file upload |
| Voice | yes (OGG) | no | no |
| Buttons | inline_keyboard | components | Block Kit |
| Reactions | emoji | emoji | emoji |
| Edit | yes | yes | yes |
| Stickers | yes | custom emoji | no |
| Embeds | no | rich embeds | no |

### File Structure

```
src-tauri/src/bridge/
  traits.rs           — OutboundContent, InboundMedia, ServiceCapabilities
  manager.rs          — dispatch by content type
  telegram.rs         — full trait impl: media download/send, callbacks, reactions
  media.rs            — NEW: download helper, temp file management
  buttons.rs          — NEW: InlineButton types, approval builder
  status_reactions.rs — NEW: auto-reactions with debounce
```

### Implementation Order

1. Define types in traits.rs (backward compatible)
2. Telegram outbound: sendPhoto/Document/Voice/Animation (multipart/form-data)
3. CLI: extend bridge.send with --type, --file, --caption
4. Inbound media: detect + download + inject path to agent
5. Inline buttons: send with keyboard, handle callback_query
6. Reactions: setMessageReaction + status controller
7. Cross-platform: prove trait with second service (Discord)
