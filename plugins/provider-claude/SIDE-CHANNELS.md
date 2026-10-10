# Claude permission hook side channel

Claude remains an interactive PTY. Decision-returning permission hooks are off
by default. Enable `agents.claude.permission_hook_decisions = true` only after
the installed owned PermissionRequest command preserves response stdout.
The provider must also declare the `claude-permission-request` capability and
the PermissionRequest hook roster entry. There is no provider-id branch.

For an authenticated, matching session and binding nonce, the daemon holds the
PermissionRequest response for at most 60 seconds. It publishes one structured
prompt and one action_needed notification with the provider's Allow and Deny
choices. The existing notification system owns bell, Telegram and push routing.
The same request receives answers from every surface; the first answer wins.
Opaque per-request option values protect notification identity against a later
request with identical labels. Numeric CLI option positions remain supported.

An answer produces only the corresponding hook decision. Timeout, close,
rebind, missing authentication or an unavailable daemon produces no decision;
Claude keeps its own permission flow. Notification hook events retain their
previous route. This channel creates no automatic answering policy.

Concurrent requests have independent identities, answers and notification records.
They do not occupy the tab-wide PendingPrompt notification condition. The composer
shows the oldest live request; notification choices can answer any live request.
Resolving or cancelling one request leaves the others pending.

## Installed-hook evidence and re-trust note

Read-only inspection on Claude Code 2.1.292 found an installed CodeTerm-owned
`codeterm-hook-v3` PermissionRequest command using `curl -s -o /dev/null`.
It forwards stdin with `--data-binary @-`, but discards the daemon response.
Its timeout is 5000 and curl has no `--max-time`. URL, token and tab id come
from environment variables; no instance address or credential is embedded.
Native and JS installer source in this lane already preserve curl stdout.

The installed hook needs a consented refresh before this channel can work.
Refreshing its command changes trusted content. The owner accepted the one-time
re-trust cost: review and re-trust the changed provider hook if Claude requests
it. Preserve foreign hooks and the user's statusLine. Keep the capability off
until the refreshed command and a real allow/deny round trip are verified.
This lane does not write the machine-global settings file or alter generator
command bytes. This note must accompany any merge or subsequent hook refresh.

The [current hooks reference](https://code.claude.com/docs/en/hooks) confirms
command-hook stdout JSON decision handling and timeout units in seconds.
It also documents `updatedPermissions` with `destination: "session"` for
in-memory grants. This implementation exposes one-request Allow and Deny only;
it does not echo suggestions or change persistent permissions or permission modes.

## Remaining verification and producer work

Rust compilation, broker/notification integration tests and native/JS parity
run in canonical. A real owner DEV oracle must prove Allow executes the tool,
Deny refuses it, timeout restores Claude's prompt and each resolution retires
the notification on every configured delivery surface.

Session-id injection remains disabled. Commit 504d26530680d66e7db2929a08bf65cd538da1a1
disabled it together with stripping inherited child-session environment values;
that combined success does not isolate the flag. A fresh interactive owner DEV
comparison, keeping environment stripping unchanged, is still required.
`claude --version` reported 2.1.292; `--help` advertises `--session-id` and
`--settings` but no Channels enabling flag. Channels are not implemented.

Context production requires `agents.claude.context_feed = true`; default is off.
For a per-launch statusline also pass `--codeterm-statusline` in provider args.
The provider consumes that private flag and passes an inline `--settings` JSON
containing only statusLine. It reads at most 65537 bytes from the user's settings,
forwards stdin to the authenticated daemon with a two-second curl deadline, then
chains the original command with the same JSON stdin. Original padding is kept.
It never writes global settings. An explicit caller `--settings` takes precedence;
malformed, oversized or unsupported user statuslines leave the plain PTY launch.
Chained command input is capped at 8192 UTF-8 bytes and padding at 1000 to keep
the inline launch argument bounded on Windows as well as Unix.

The consumer wire location is `tab.context` on the existing mux snapshot:
`{used_tokens, window_tokens, cost_usd, source}`. Tokens are integer counts and
cost is USD or null. Source is `claude-statusline` or `claude-jsonl`. Only those
four bounded numeric/provenance fields survive normalization; cwd, paths, raw
model metadata, session ids and credentials do not enter the context wire.
Statusline HTTP input is capped at 65536 bytes. Every write checks the tab's own
session id and binding nonce. Runtime model/session clearing also clears context.

Initial JSONL snapshots and live deltas extract message.usage through the existing
plugin reader. Each record replaces current input usage; it is not a sum across
turns and excludes output tokens. JSONL usage alone has no authoritative window
size: retain a reported window, or explicitly configure the provider's
`context_window_tokens`. With neither, window_tokens is zero and the meter hides
the denominator. No window is guessed from a model name or subscription.
The [statusline reference](https://code.claude.com/docs/en/statusline) defines
current context usage from input plus cache-read plus cache-creation tokens,
excluding output tokens; current_usage can be null after compaction.
