# Claude side-channel verification evidence

Moved from the bundled provider guide; this records repository verification
and a consented local hook refresh, not permission for another installation.

## Installed-hook evidence and re-trust note

Read-only inspection on Claude Code 2.1.292 found an installed CodeTerm-owned
`codeterm-hook-v3` PermissionRequest command using `curl -s -o /dev/null`.
It forwards stdin with `--data-binary @-`, but discards the daemon response.
That installed hook used a timeout of 5000 seconds; installers now use 5 seconds.
Curl has no `--max-time`. URL, token and tab id come
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

