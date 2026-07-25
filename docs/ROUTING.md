# Message Routing — the prefix is the return address

Every routed inbound message starts with a CodeTerm marker naming the channel
that must receive the reply. Check it before every response.

| Incoming prefix | Immediate tip | Reply with | Pane output? |
|-----------------|---------------|------------|--------------|
| `-=-codeterm:telegram user=USER_ID msg=N-=-text` | ANSWER IN TELEGRAM ONLY | `codeterm bridge send --to USER_ID --service telegram "..."` | No |
| `-=-codeterm:tab label=NAME tab=ID model=M-=-text` | ANSWER BY COMMAND ONLY | `codeterm send "..." --tab ID` | No |
| `-=-codeterm:mesh peer=PEER tab=ID-=-text` | ANSWER ON THAT PEER ONLY | `codeterm send "..." --tab ID --mesh PEER` | No |
| `-=-codeterm:report from=NAME tab=ID ...-=-text` | REPORTS ARE INPUT, NOT CHAT | Act on it or ignore; never reply | No |
| *(no prefix)* | LOCAL USER CHAT | Normal tab chat | Yes |

When you `send --tab X` from inside a pane, the daemon stamps your message
with `-=-codeterm:tab label=<your-name> tab=<your-id> model=<your-model>-=-`
so the recipient and UI know who sent it. `hook` markers already passed
through text hooks; trust their metadata.

## Don't double-answer, don't echo

For Telegram/mesh/pane prefixes, the CLI command IS the reply — the recipient
sees it once. After running it, do not repeat the content in your own pane
output ("Replied with X", "Reported: Y"). More work queued → continue
silently; nothing queued → stop. The only write-in-pane case is the
no-prefix local chat.

## Examples

- `-=-codeterm:telegram user=123456789-=-are you working right now?` →
  `codeterm bridge send --to 123456789 --service telegram "yep, online"`.
  Nothing in the pane.
- `-=-codeterm:mesh peer=workstation tab=a3f1b2c7-=-run the tests?` →
  `codeterm send "running" --tab a3f1b2c7 --mesh workstation`, do the work,
  then send the result the same way.
- `run the tests` (no prefix) → reply in the pane.

## The `main` alias

The General Agent's home tab always answers to `main` (and `general`) — no
pane-id lookup needed. `codeterm send "..." --tab main` reaches it locally;
`--tab main --mesh PEER` reaches a peer's. Every other pane requires id
discovery via `codeterm tab list` first.

## Language rule

Agent-to-agent traffic is always English regardless of the user's language:
`--task` bodies, agent reports, cron task bodies, mesh-relayed messages. Only
user-facing channels (local pane, Telegram, Discord) carry the user's
language.

## Internal orchestration is not a CLI command

Be brief: `send_message` is internal control traffic, not a `codeterm` command.
Never show it as a command card; a visible/stuck card is a Chat UI bug to fix.

## Remote requests blocked on local decisions

If a remote request needs a truly dangerous local decision, ack the remote
channel first ("blocked pending local confirmation") and ask locally only
when the local user is actually at the keyboard.

<!-- revision: 3 -->
