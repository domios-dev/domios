# mem.search.basic — opencode

**Prompt:** Search our memory for anything about the Telegram bridge and summarize what you find.

- **user:** Search our memory for anything about the Telegram bridge and summarize what you find.
- 💭 _The user wants me to search memory for information about the Telegram bridge and summarize what I find. Let me search the memory system to find relevant information._
- 🔧 **bash** `codeterm mem search "telegram bridge"`
- 💭 _Let me get more details on the top results, especially the most relevant ones. Let me also search for "bridge" more broadly and look at the Codeterm skill which likely has bridge details._
- 🔧 **bash** `codeterm mem search "bridge telegram codeterm"`
- 🔧 **bash** `codeterm mem get skill_send_telegram_message_via_bot`
- 🔧 **bash** `codeterm mem get fact_telegram_webhook_endpoint`
- 🔧 **bash** `codeterm mem get skill_telegram_command_handler`
- 💭 _Let me also search for the bridge reference doc and any related graph connections._
- 🔧 **bash** `codeterm mem search "bridge migrate telegram"`
- 🔧 **bash** `codeterm mem graph related --id service_telegram`
- 💭 _Now I have a comprehensive picture of the Telegram bridge in memory. Let me summarize everything I found for the user.

Key findings:
1. **CLI bridge commands** - `codeterm bridge send` for replying v_
- **assistant:** Here's a summary of everything in memory about the **Telegram bridge**:

**CLI interface** — `codeterm bridge send` is the primary command to reply via Telegram from any pane:
- Normal: `--to USER_ID --service telegram "text"`
- Append mode: `--append` concatenates onto the previous message (within 60s, useful for streaming long replies)
- Media: `--type photo --file <path>`
- Long answers should be split into multiple `send` + `--append` calls to stay under Telegram's 4096-char limit

**Inbound
