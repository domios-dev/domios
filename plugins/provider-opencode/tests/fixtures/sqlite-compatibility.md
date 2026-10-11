# OpenCode SQLite compatibility evidence

Compared upstream tags `v1.18.35` (`53d1eabb61e21162157817bf677da0a4ad3332e3`) and `v1.19.0` (`733b2c627e5aec39cb33cc22373bdce33053ef2c`). `git diff` between them is empty for these consumed storage contracts:

- `packages/core/src/database/` (migration implementation and every migration).
- `packages/core/src/session/sql.ts` (session, message and part columns).
- `packages/core/src/global.ts` (XDG data location).
- `packages/schema/src/v1/session.ts` (user/assistant messages, text/reasoning/tool parts and tool states).
- `packages/opencode/src/session/message-v2.ts` and `packages/opencode/src/cli/cmd/import.ts` (persistence import).

Source: [1.18.35 table definitions](https://github.com/sst/opencode/blob/v1.18.35/packages/core/src/session/sql.ts), [1.19.0 table definitions](https://github.com/sst/opencode/blob/v1.19.0/packages/core/src/session/sql.ts), [database paths](https://github.com/sst/opencode/blob/v1.19.0/packages/core/src/database/database.ts), [JSON schemas](https://github.com/sst/opencode/blob/v1.19.0/packages/schema/src/v1/session.ts).

The adapter consumes `session.id/directory/title/time_created/time_updated`, `message.id/session_id/time_created/time_updated/data` and `part.id/message_id/session_id/time_created/time_updated/data`. All remain unchanged. Message JSON still carries `role`, assistant `providerID/modelID/tokens/cost` and user `model`; parts still carry `type/text` or `tool/callID/state`. Import strips message and part identity fields into their SQL columns. Named-column readers tolerate column ordering differences between fixture dumps.

Released CLIs use `$XDG_DATA_HOME/opencode/opencode.db` (default `~/.local/share/opencode/opencode.db`), with absolute or data-relative `OPENCODE_DB` overrides; `:memory:` cannot be read externally. Upstream development channels can use a channel-suffixed database; that pre-existing path limitation is outside the verified release range.

`sqlite-1.19.0.sql` captures the schema and rows actually persisted by an isolated `opencode-ai@1.19.0` npm installation. Synthetic user/assistant/tool input was imported using `opencode import <file> --pure`; `OPENCODE_DB` and all four XDG homes pointed into a scratch directory. No global binary or real session was modified. Project/directory paths were redacted to `/fixture`; payload was synthetic before import. The existing 1.18.35 fixture remains independent.

The old exact-version gate protected against assumed schema conversion, event-route and server-auth changes, declared breaking for 2.x. It also discarded compatible undeclared patch/minor releases before preparing any SQL. Verified 1.19.x is now supported; undeclared detected versions stay unverified but read through sandbox confinement, SQLite read-only opens, prepare-time column checks, size limits and JSON/type parsers. Known breaking versions and failed/pending version probes remain blocked. Settings > Agents exposes the verdict without notifications or a chat banner.

Focused evidence: `modelSwitch.test.ts` exercises both persisted stores through built `plugin.js`, own-session filtering and cursor exhaustion. Rust `sqlite_fixture` adds the real host version gate, future-version best effort and incompatible table shape; run it in canonical, never a cold lane.
