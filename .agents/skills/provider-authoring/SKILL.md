---
name: provider-authoring
description: "Author and validate JavaScript agent provider plugins for Domios."
---

# Authoring an Agent Provider Plugin

`launchOnboardingSafeChoice(screen)` declares the exact option value associated
with `launchOnboardingResponse(screen)`. Missing or null means unsupported. The
host requires a unique provider-parsed option, a delegated Worker/Planner role,
and current launch/prompt evidence before consuming the bound next key. It never
derives that value from the response's opaque frame key.

For model-bound update responses, a JS `buildPromptResponse` implementation
declares live-focus navigation; native providers declare it through
`live_prompt_response`. The generic arrow-down default is unsupported
for this automatic path. The host consumes one returned step, then re-observes.

How to build a JS drop-in agent provider for Domios — the manifest schema, the
JS exports the host calls, what falls back to what, and what tests you owe.
General plugin anatomy (build pipeline, QuickJS sandbox, `host.*` API, the
capability registries) lives in [PLUGINS.md](../PLUGINS.md); this doc covers only
the `agentProvider` capability.

Every claim below is transcribed from the Rust source of truth:

- manifest types — `src-tauri/crates/codeterm-plugin/src/js_runtime/manifest.rs`
  and `manifest/config.rs`
- the JS bridge — `src-tauri/crates/codeterm-plugin/src/js_runtime/provider.rs`
  (`JsProvider`)
- the full provider contract — `src-tauri/crates/codeterm-plugin/src/provider.rs`
  (`AgentProvider` trait)

## The two-surface contract

`spawn.paste.supportsBracketedPaste` defaults to true. Set it to false for
plain input that cannot interpret bracketed paste; the refusal overrides live
terminal mode, multiline TUI framing and typed prefixes. Declare
`spawn.paste.multiline: "tagged_block"` for plain readers supporting
`{tag` and `tag}` lines. Core wraps the entire input frame with a collision-free
tag at the shared PTY submit seam, including typed envelopes. Chat, CLI, reports
and launch input all use this declaration; do not duplicate transport wrapping
in a chat preprocessor. Plain frames otherwise normalize newlines to carriage
returns. `typedFollowUp` is a second bracketed paste in the same write,
sent only when the body itself is bracketed. A paste-only declaration may omit
the follow-up fields; it adds no sandbox host function.

`authStatusProbe()` declares bounded subprocess arguments; `parseAuthStatus(record)`
decodes its supported JSON. `parseSessionObservations(record)` carries typed API
errors and permission mode from the claimed transcript. Native and JS paths owe
parity fixtures. The host runs probes on a worker thread and consumes observations
only when the provider's `structured_signals` setting is enabled (default false).
No new sandbox host function is exposed by these capabilities.

`readStructuredChat` may return `observations` alongside its Chat rows: session-scoped
RichMessages carrying `provider_observations`. The host consumes them on initial
Chat reads, live deltas, provider error events and heartbeat reads. An exact own
session is required; `unavailable_model` removes a confirmed non-retryable model
failure from the cached catalogue. Assistant API errors render as `system` rows
with `systemSubtype: "api_error"`. A successful completed assistant clears the
API error. No new sandbox host function is needed.

`modelCatalogueSeed()` decorates the manifest seed once at provider registration;
it must be pure and must not probe a binary or network. Per-model effort launch
mapping uses `reasoningEffortLaunchArgsForModel(model, effort)` with native parity.
Claude's `model-capabilities.json` owns effort maps and compact windows separately
from CLI version compatibility. `--codeterm-model-capabilities` explicitly opts
launch/resume into its per-launch compact-window environment overlay; absent that
flag, the provider's automatic compaction policy remains authoritative.

`spawn.forkSessionArgs` declares session forking. Core opens a separate tab,
resumes the source with those arguments, and accepts only a distinct session
proved by the new launch's nonce. No session id is prebound for a fork.
`tracksBackgroundWork()` plus `parseBackgroundWork(record)` declare structured
background task observations. The bounded per-tab roster is session-scoped;
`background_work_guard` defaults false. When enabled, restart refuses live work,
provider updates wait for a roster change event, and explicit force is reported.

A JS provider never sees the ~70-method Rust `AgentProvider` trait. It sees two
surfaces:

1. **`plugin.json`** — pure data. Spawn semantics, session file layout, command
   templates, models. Parsed strictly (`deny_unknown_fields`): a typo'd key fails
   the load, visible in `/api/plugins/snapshot` or daemon logs.
2. **`plugin.js` exports** — logic. Every export is optional; a missing export
   falls back to the manifest, a trait default, or (for bundled drop-ins only)
   the displaced native provider of the same id. Third-party providers have no
   native fallback — for them "native fallback" rows below read as the stated
   default.

The typed authoring surface is `@codeterm/plugin-sdk` (type-only, on npm).

### `hostApi` — declare it or be skipped

Third-party manifests must declare `hostApi` matching the running build's
`HOST_API_VERSION` (currently **2**, `js_runtime/host_api.rs`). A manifest
without it (or with an unsupported value) is **skipped at daemon load** — not
rejected at install — so always pin it. First-party bundled plugins are exempt.

### Launch-binary availability

The availability shown by provider enumeration is a live capability observation,
not a provider-name rule. The shared resolver checks the daemon's inherited
process `PATH`, then the current Windows machine and user `PATH` values read
from the environment registry, and finally the configured rescue directories.
The same ordered roots are returned in the `searched` diagnostic. Registry
reads and expandable values are handled in-process; the resolver never changes
the process-wide environment.

On Windows, a bare launch-binary name is searched only with the configured
`PATHEXT` suffixes; an extensionless regular file is not a PATH or rescue-root
candidate. An explicit or relative path containing a separator is checked
directly as supplied, including an extensionless path.

`available` means a runnable regular file was observed, `unavailable` means a
readable search completed without finding one, and `unknown` means the search
could not answer (for example, an environment read failed). A missing result
is not retained across observations, a found path is rechecked before reuse,
and a changed root set creates a fresh observation. This lets an installed CLI
become available without restarting the daemon and prevents a removed file from
remaining available. Preflight, enumeration, and `host.exec` use this shared
observation; Windows PTY launch preserves the same process → machine → user
PATH precedence without mutating the caller's environment.

### CLI status and management

`checkForUpdate()` exports `{ currentVersion, latestVersion }` or null through
its existing provider capability. Declare the probe binary in `subprocess.allow`
and metadata host in `network.allow`; there is no manifest `checkForUpdate` key.
Providers with breaking ranges select the newest supported non-breaking target
and use `{version}` in their manifest updater. The host validates and substitutes
that exact version for both automatic updates and `agent cli update`.

The host's shared CLI status service uses `cliBinary`, falling back to
`binaryName` and then the manifest id. Spawn preflight checks the actual launch
binary. Status includes the resolver verdict and path plus `installer` and
`updater` for the current OS, without changing command content. Chat backends
explicitly require no CLI. Missing and unknown observations stay distinct.

Plugin install/update prints that status immediately. Agents can inspect it with
`codeterm agent cli status <provider>` and run the declared commands with
`codeterm agent cli install <provider>` or `codeterm agent cli update <provider>`.
Commands run in visible Team utility tabs, with completion reports and status
refreshes; unsupported OS commands are refused before a tab is created.
Status and dispatch warnings name missing command runners with setup guidance,
derived from the manifest command's leading executable or explicit shell
availability checks. This diagnostic does not parse arbitrary shell programs or
install prerequisites; conditional installers can use an available alternative.

## Minimal vs full: pick your tier

The contract supports a thin provider; nothing forces codex-grade depth.

- **Minimal (launch + detect + generic status)** — ~5 artifacts, roughly 400
  authored lines: `plugin.json`, `package.json`, a small `src/plugin.ts`
  (`buildLaunchCommand`, `buildResumeCommand`, `detectFromTitle`,
  `detectFromOutput`), the built `plugin.js`, and the install loop
  (`codeterm plugin install --from-dir <dir>`). Everything else degrades sanely:
  detection falls to the universal prompt detector, session discovery to the
  manifest-driven heuristic, status to the generic classifier, chat UI honestly
  absent. This tier works today.
- **Codex-grade (chat UI, tab-state classification, usage parsing, prompt
  navigation)** — all ten artifact classes, ~3,700 authored lines plus screen
  fixtures plus (for bundled drop-ins) Rust parity cases. Reference:
  `plugins/provider-codex/` (117-line manifest, 2,103 lines of TS across 9
  files, 1,536 lines of tests, 17 fixtures). The thinnest full drop-in is
  `plugins/provider-cursor/`.

Only two capabilities have **no manifest expression** and therefore require JS
even at the minimal tier: `buildLaunchCommand` (a missing export returns an
empty string and the launch breaks) and `buildResumeCommand` (missing means no
resume).

## Manifest reference

Top-level `plugin.json` fields shared with capability plugins (`id`, `version`,
`displayName`, `description`, `configHelp`, icons, `entry`, `capabilities`,
`settingsSchema`, `requiresReload`, …) are documented in
[PLUGINS.md](../PLUGINS.md). Provider-relevant top-level fields:

| Field | Type | Meaning |
|---|---|---|
| `binaryName` | string? | Process name used for type-mapping a PTY to this provider. Defaults to `id`. |
| `cliBinary` | string? | Binary probed for install/version checks. |
| `cliCompatibility` | `{supported: string[], breaking: {range: string, reasons: string[]}[], modelMinimumCli?: map, directSqlite?: bool}` | Rust semver ranges (comma-separated comparisons); empty supported ranges explicitly mean unknown. Breaking ranges win. `directSqlite` defaults false and gates read-only SQLite access: known breaking, invalid and undetected versions are refused; detected undeclared versions retain an unknown verdict and use best-effort SQL/JSON shape checks. Settings shows unverified compatibility quietly. |
| `installer` | map platform→string | Per-OS install command (`macos` / `linux` / `windows`). |
| `updater` | map platform→string | Per-OS update command run silently by the coordinator. `{version}` pins the checked target; required when compatibility declares breaking ranges. Running agents keep running. |
| `models` | `[{id, displayName, description?, available?}]` | Model seed registered with the model catalog. Each entry carries `id`, `displayName`, optional `description`, optional `available`; `available: false` ships an alias disabled until live discovery confirms it. |
| `modelCatalogue` | `{authority: "authoritative"|"free_form", liveDiscoveryRequired?: bool}` | `authoritative` closes model selection to catalogue ids; `free_form` preserves provider-owned ids. Set `liveDiscoveryRequired` when the seed is not a trustworthy validation list. If no live snapshot is available, Domios preserves the requested id and returns a warning. |
| `modelGroups` | `[{id, label}]` | Provider-owned groupings of `models` for the picker (e.g. the upstream company a routed model belongs to). `id` matches a model-id prefix, `label` is the text the picker renders; core never maps a vendor id to a label. Empty ⇒ one flat list. |
| `workerLaneSettings` | `[{path: string, patch: object}]` | Project settings files merged into every Worker lane at provisioning, for example to keep the provider's own language-server plugin from building there. `path` is lane-relative and may not escape the lane; JSON files merge, `.toml` files merge as TOML. The files stay ignored and unstaged in the lane. Require the first host version supporting this declaration. |
| `credentials` | `[{id, file, fileEnvironment?, public?, secret?}]` | Credential files the host reads on the plugin's behalf. Declaring one also revokes the plugin's own read access to that file. A declared file's fingerprint is captured with the cached model snapshot, so connecting a provider (the file appearing or changing) re-probes its models without a rebuild. |
| `defaultModel` | string? | Default model id. |
| `reasoningEfforts` | `[{id, displayName, description?}]` | Reasoning-effort levels. |
| `defaultReasoningEffort` | string? | Default effort id. |
| `spawn` | object | Spawn semantics — table below. Defaults apply when the whole block is absent. |
| `sessions` | object? | Session-file layout — table below. |
| `directoryTrustRecord` | `{format: "toml_folders", path: string}`? | Host-owned folder pre-trust: sets `folders.<canonical launch cwd>.trusted` and `decided_at` in TOML. The absolute/expanded file must sit directly in the declared sessions root's parent. No filesystem access is granted to plugin JS; require the first host version supporting this declaration. |
| `commands` | object | Command/key templates — table below. |
| `integration` | object? | Hook integration declaration — table below. |
| `rule` | object? | One field, `path`: where the codeterm-rule markdown file lives (e.g. `~/.claude/CLAUDE.md` sibling). Drives the manifest fallback of `installBridgeRule` / `uninstallBridgeRule`. |
| `skills` | object? | One field, `dir`: where plugin-contributed skill files get written. Parsed, but skill install/uninstall is **not bridged to JS** (see caveats). |
| `usage` | object? | One field, `probeCommand`: slash-command that probes usage in the live TUI (e.g. `/usage`). |
| `permissions` | object | Sandbox contract — table below. |

### `spawn`

When a `spawn` block is present, `readinessStrategy`, `taskDelivery`, and
`verifySubmit` are **required**; every other field defaults.

| Field | Type / legal values | Default | Meaning |
|---|---|---|---|
| `readinessStrategy` | `{"kind":"poll_shell_prompt","timeoutSecs":N}` \| `{"kind":"fixed_delay","durationMs":N}` | `poll_shell_prompt` / 30 s | When the PTY is ready to receive the launch command. |
| `taskDelivery` | `"inline_flag"` \| `"pty_input"` | `pty_input` | Whether the task rides the launch command line or is typed into the PTY after launch. |
| `systemPromptDelivery` | `{"kind":"external"}` \| `{"kind":"cli_flag","flag":"--append-system-prompt"}` \| `{"kind":"session_config"}` | `external` | How the composed system prompt reaches the agent. |
| `providerReadiness` | `{"kind":"tui_frame","timeoutSecs":N}` \| `{"kind":"structural_composer","timeoutSecs":N}` | `tui_frame` / 30 s | When the agent TUI itself counts as ready (post-spawn). |
| `composerCursorPosition` | `"bottom_region"` \| `"visible_anywhere"` | `bottom_region` | Where the composer cursor appears. `visible_anywhere` allows lazy-session readiness to use a visible cursor anywhere on the provider's alternate-screen TUI. |
| `verifySubmit` | `{"kind":"shell_prompt","timeoutSecs":N}` \| `{"kind":"tui_input_drained","initialKeyDelayMs":N}` \| `{"kind":"none"}` | `shell_prompt` / 5 s | How the host verifies a submitted message actually went in. `initialKeyDelayMs` defaults to 0. |
| `busySubmit` | `{"kind":"submit_normally"}` \| `{"kind":"key","key":"<bytes>"}` | `submit_normally` | What to send when submitting while the agent is generating. |
| `sessionIdInjection` | `"none"` \| `"fresh"` \| `"fresh_and_fork"` | `none` | Whether the host generates and injects a session id at launch (and on fork). |
| `statusBarRows` | u16 | 0 | TUI status-bar rows to exclude from output scraping. |
| `agentStateStatusReconciliation` | bool | false | Provider participates in agent-state ↔ status reconciliation. |
| `supportsLazySkills` | bool | false | Provider can lazy-load skills via trigger words; otherwise skills merge into the system prompt up-front. |
| `hasHooks` | bool | false | Provider has hook-based detection. |
| `sessionStartsOnFirstUserTurn` | bool | false | The provider emits its session binding only after its first user turn; launches without a startup task accept input from declared composer readiness. |
| `hookEventRoster` | `[{event, route}]`? | absent | Declared hook events: each entry names an `event` and a core ingestion `route`. Unknown routes fail the **whole roster** closed. Absent ⇒ native fallback's roster, else SessionStart-only when `hasHooks`. |
| `interactiveEvents` | bool | false | Plugin exports `parseInteractiveEvents`; host calls it to extract interactive prompts. False ⇒ native fallback handles it. |
| `supportsPromptAnswers` | bool? | Native capability, otherwise true | Controls Chat question extraction and answer cards independently of terminal picker classification and trust parsing. Explicit false overrides native fallback. |
| `supportsStructuredPrompts` | bool | false | Provider exposes a structured prompt source; host builds `pending_prompt` from it instead of scraping the PTY. **JS-only providers cannot serve this** — see caveats. |
| `structuredPromptServer` | bool | false | The structured handle is a local server the provider must bind; host allocates `127.0.0.1:port` and passes it as `structuredPromptServer` at launch. Pull-handle providers leave this false. |
| `usageProbeCommand` | string? | absent | TUI command that prints the usage screen (e.g. `/usage`, `/status`). Pairs with the `parseUsage` export; the usage poller skips providers without both. |
| `launchPermissions` | object? | absent | Launch-scoped grants applied through launch args, never global settings files. `unattendedModeModels`: models whose default mode runs tools unattended without full access (empty ⇒ no claim). `commandRuleFormat` (`{command}`) + `allowRuleArgsFormat` (`{rule}`): how one Domios command prefix is allowed. `denyToolArgsFormat` (`{tool}`) + `nativeSubagentTools`: how managing roles lose native subagent tools. Without full access, a model outside `unattendedModeModels` gets the role's Domios command rules, and a managing role (Orchestrator, Planner) on such a model is refused before spawn. |

### `sessions`

Drives the manifest fallbacks for session discovery (`sessionFilePath`,
`sessionExists`, `detectSessionId`, `enumerateSessions`, `findSession`).

| Field | Type / legal values | Default | Meaning |
|---|---|---|---|
| `rootDir` | string (required) | — | Filesystem root the provider writes session files into. `~/` expands to the home dir. E.g. `~/.claude/projects`. |
| `cwdEncoding` | `"dash_encoded"` \| `"none"` | `dash_encoded` | How a cwd maps to a subdirectory name: `dash_encoded` turns `/Users/foo/code` into `-Users-foo-code`; `none` uses the path as-is. |
| `fileExtension` | string | `"jsonl"` | Session file extension. |
| `searchFiles` | object? | absent | Search-only filesystem enumeration. `rootSubdir` selects a path beneath `rootDir`, `directoryDepth` bounds the number of directories to descend (default 1, host maximum 8), and `fileStemRegex` optionally extracts the session id from capture group 1. Search reads file paths and timestamps in Rust, then asks the provider for descriptive fields only after the raw content prefilter selects a candidate. |

### Session identity confirmation

`commands.modelSwitchCommands.confirmationSource: {kind: "session_identity"}`
selects provider-owned model metadata instead of transcript stdout. Declare
`model: {command: "/model {model}"}` and optional
`effort: {command: "/effort {effort}"}`. Export
`sessionModelIdentityPath(cwd, sessionId): string | null` for the exact session.
The host canonicalizes that existing file beneath the permitted session root,
installs a parent watcher before writing, and requires changed own-file bytes
plus the target from `detectSessionModel` or `detectSessionReasoningEffort`.
An already-current value is a no-op. Missing paths or unchanged evidence fail
honestly. Session, PTY and binding nonce remain fixed; no automatic restart.
The default `confirmationSource: {kind: "transcript"}` preserves the existing
`transcript` declaration and fresh wrapped-stdout confirmation.
No new sandbox host function or filesystem grant is exposed.

Busy model changes require authenticated own-turn completion; Idle alone never
releases a staged change. Providers without hooks may declare
`commands.modelSwitchCommands.turnCompletion` with `eventPointer`, `eventValue`,
`sessionPointer`, `startEventValue`, `turnPointer`, `activeTurnPointer` and `source`.
Pointers use JSON Pointer syntax against records in the exact bound session's
`sessionJsonlPath` (`source: "session_jsonl"`) or `sessionFilePath`
(`source: "session_file"`). Select the file that contains the declared completion
records; chat and update files can differ. Both paths require session-root
confinement. The host captures an append boundary, observes file events,
and matches the completed turn id against the latest provider-authored active
turn id. A new user record invalidates prior completion evidence. Missing,
foreign, partial or pre-boundary records cannot release the staged operation.

Next-turn hooks read `GET /api/tabs/{id}/turn-controls?session_id=...` with the
instance token and `x-codeterm-binding-nonce`. The exact-session response contains
`model` and `effort`, preferring a staged selection over resolved launch values.
It does not change actual identity or confirm a switch. Binding and controls
requests need independent failure deadlines. Unavailable or mismatched controls
must preserve the user message and current model, warn once quietly, and leave
the selection pending for a subsequent turn; hooks must never block the message.

`parseUsage` snapshots carry `billing_mode` (`subscription`, `api`, `unknown`)
and `limits_reported` (boolean). Unknown is the default. Subscription snapshots
with no reported metrics display that limits were not reported, without guessed
spend. Providers must preserve missing meters as null.

### `commands`

All fields optional.

| Field | Type | Meaning |
|---|---|---|
| `compact` | string? | In-agent compact command (e.g. `/compact`). |
| `freshContext` | string? | In-agent fresh-context command. |
| `exit` | string? | In-agent clean-exit command. |
| `idleRestartTermination` | bool | Transcript is durable while idle, so a verified idle restart may terminate without the exit command. Default `false`; busy restart behavior is unchanged. |
| `stopKey` | string? | One raw key value for interrupting the current generation. |
| `stopKeys` | string[]? | Ordered raw key values for a multi-key interrupt sequence. |
| `stopKeyWaitForOutput` | bool | Wait for a PTY screen change between sequence keys. Default `false`. |
| `modelSwitch` | `"restart_resume"` \| `"live"` \| `"live_command"` \| `"next_turn"` \| `"unsupported"` | `next_turn` keeps the session running: an RPC endpoint receives per-turn controls; a provider hook consumes the authenticated tab's resolved controls before persisting its user message. Hook providers require `sessionModelTranscriptBoundary` and `detectSessionModelAfterBoundary`; the shared core stages busy requests and confirms only a new assistant record. Unavailable boundaries refuse without restart. `live` uses the tab's advertised model route. `restart_resume` resumes the same verified session with new launch args and waits for post-boundary assistant evidence. `live_command` runs a declared idle command and confirms fresh bound-session output; failures never restart automatically. Default `unsupported` refuses without writing. |
| `modelSwitchCommands` | `{model: {command, confirmation}, effort?: {command, confirmation}, transcript: {recordType, contentPath, prefix, suffix, ignoredContentRegexes?}}` | Required for `live_command`. Commands substitute `{model}` or `{effort}`; confirmation regexes also accept `{display_name}`, with selection values regex-escaped. Transcript declarations select the JSONL record type, string content path, and exact output wrapper. Optional `ignoredContentRegexes` identify complete non-turn command records such as caveats and exact command echoes; the inter-command guard excludes only whole-content matches or the exact stdout wrapper, retaining mixed prompt content. Only fresh output in the tab's own bound session confirms a switch. Busy tabs refuse before writing; a timeout fails without replay or restart. The picker offers a separate, confirmed `restart_resume` fallback. |

A live command can declare `confirmationPromptKind: "switch_confirmation"` and export
`parseSwitchConfirmationPrompt(screen, selection)`. This pure structural classifier
returns the current two-option confirmation with its confirming option first, or
null. It must reject unrelated, stale and ambiguous frames without matching vendor
copy. The host additionally verifies its options against `parsePrompt` and uses
`buildPromptResponse` for that exact frame. No new sandbox host access is granted.

The observer is armed only when Domios queues that declared slash command, including
Chat writes. A bare picker command supplies the requested selection from the switch
operation. A later payload cancels the observer. One response is permitted, and only
fresh declared output from the same session confirms the switch; no prompt means no
keys. Screen and own-file events drive observation within a failure deadline.
This declaration supersedes the legacy regex prompt response for that command.
Native and JS implementations owe shared classifier parity fixtures.

Each live command may declare `prompt: {screenRegex, responseKeys, cancelKeys?}`. The screen
regex accepts the same escaped placeholders; response keys are literal UTF-8
strings (for example `"\r"`), concatenated into one bounded write. The host
refuses a matching prompt before injection and answers once only after its own
command was written, under the same session, PTY and binding nonce. The current
screen must match at response enqueue. A response never confirms success: the
declared transcript output must still arrive before the failure deadline.
On failure, optional literal `cancelKeys` dismiss only the matching dialog caused
by this command; the host waits for that dialog to clear before returning.
An effort-only selection sends only its effort command and preserves model identity.

A live command may declare a session-only `picker` with `screenRegex`,
`selectedRegex`, `targetRegex`, `position` (`number` or `column`), `previousKey`,
`nextKey`, `responseKey`, and `cancelKey`. Both selection regexes capture
`position`; numeric pickers capture the displayed option number, while column
pickers capture the text before the cursor or target label. Column targets also
capture `label`. Core writes one navigation key per changed cursor position,
re-observes the exact owned picker, and writes `responseKey` only on the target.
Missing targets refuse with the explicit restart-and-resume fallback. Bare slash
commands can open pickers without a selection argument. No new host API or
sandbox access is granted; native and manifest declarations owe parity.
Transcript appends wake confirmation alongside screen changes. Picker responses
never establish success; fresh declared stdout in the owned session does.
| `autoApproveFlags` | string[] | Launch flags for permission-bypass mode (e.g. `--yolo`, `--force`). Empty ⇒ the UI hides the bypass choice (`supports_permission_bypass` is derived). |
| `modelLaunchArgsFormat` | string[] | Launch-arg template, each element may contain `{model}`. Empty ⇒ default `--model <id>`. |
| `reasoningEffortLaunchArgsFormat` | string[] | Launch-arg template with `{effort}`, checked against the installed CLI's own `--help`. Absent ⇒ no scriptable effort flag, and spawn refuses any `--reasoning-effort` for this provider; declare no `reasoningEfforts` then. |
| `additionalWritableDirFlag` | string? | CLI flag accepting one extra writable directory. For a sandboxed linked worktree, core supplies the lane gitdir plus the canonical project's `objects`, `refs`, and `logs` directories; it never supplies the common `.git` directory. Applied on spawn, dispatch, restore, and resume after validating the worktree against its canonical project. |

### `integration`

| Field | Type / legal values | Meaning |
|---|---|---|
| `kind` | `"claude_settings_json"` \| `"provider_managed"` \| `"plugin_file"` \| `"none"` | `claude_settings_json`: edit a JSON file's `hooks` object with curl-back commands. `provider_managed`: provider JS owns a reviewable config merge + backup/restore. `plugin_file`: provider owns a marked plugin file that core may refresh only while its ownership marker remains. `none`: PTY parser does it all. |
| `settingsPath` | string | Where the integration is stored (e.g. `~/.claude/settings.json`). |
| `trust_grant` (alias `trustGrant`) | object? | One field, `managedKeys` (string[]): managed hook keys core passes to `grantIntegrationTrust` per install. |

`spawn.restoresConversationHistoryOnResume` declares that a resume restores the
existing conversation. Absent/false is non-restoring. A task-bound tool-less
Worker then receives one host-restored brief after authenticated rebinding,
subject to exact current-session evidence and a persisted launch claim. Resume
context includes `cwd`, `contextFiles`, and `toolLessInstructionsPath`; mandatory
attachments are revalidated and shared instructions are materialized again.

### `permissions`

Provider-relevant subset (full contract in [PLUGINS.md](../PLUGINS.md)):

| Field | Type | Meaning |
|---|---|---|
| `transportEvents` | string[] | Transport event streams the plugin may subscribe to. |
| `sendText` | bool | May write text into tabs. |
| `network` | object | `host.fetch` host allowlist: an `allow` array of hosts. |
| `subprocess` | object | `host.exec` binary allowlist: an `allow` array of binaries. |
| `secrets` | bool | May use the per-plugin secret bucket. |
| `agent_commands` (alias `agentCommands`) | bool | Spawned agents may route `codeterm plugin <id> <verb>` to this plugin's `onAgentCommand`. Declare the verbs as top-level `agentVerbs` (usage strings such as `send <chat-id> <text>`) and optional `agentKeywords`; Domios publishes them as the `plugin-<id>` mem skill and in `codeterm capabilities`. Without `agentVerbs`, the backticked spans after `Agent verbs:` in `configHelp` are used. Optional top-level `agentGuide` names a bundle-relative markdown file (absolute or escaping paths are refused; past 16 KiB it is cut with a truncation marker) that is appended to that mem skill and returned by `codeterm plugin get <id>`. Optional top-level `agentPromptLine` (one line, at most 240 characters) is added to every agent's launch prompt under `Built-in plugins:`, but only for enabled, hash-verified core plugins; a core plugin's `agentGuide` is also served by `codeterm docs <ID>`. A successful result may include `catalogueChanged: true` when the verb changed this provider's model catalogue. |
| `manage_workspaces` (alias `manageWorkspaces`) | bool | May create/manage workspaces via `host.workspace*`. |
| `spawn_agents` (alias `spawnAgents`) | bool | May spawn/drive agents via `host.agent*`. |
| `analytics` | bool | May read machine-wide KPI aggregates via `host.analytics`. |

## JS exports the bridge calls

Every export is optional (except that a provider without `buildLaunchCommand`
cannot launch). "Native fallback" applies only to bundled drop-ins that
displaced a native Rust provider; third-party plugins get the listed default
instead.

### Launch & resume

| Export | Called with | Returns | When missing |
|---|---|---|---|
| `buildLaunchCommand(params)` | `{agentName, task, starterPrompt, starterPromptText, starterPromptPrefix, args, skipPermissions, sessionId, structuredPromptServer}` | full shell command string | **empty string — the launch breaks.** The one export every provider must ship. |
| `buildResumeCommand(sessionId, skipPermissions)` | session id, bool | command string or `null` | `None` — resume unavailable. |
| `buildResumeCommandWithContext(params)` | `{sessionId, skipPermissions, systemPrompt, args}` | command string or `null` | plain `buildResumeCommand` + appended `args`; the system prompt is never re-applied by accident. |
| `starterPromptText(prompt)` | starter kind: `"assigned_task"` \| `"no_starter"` \| `"idle_wakeup"` \| `"team_bootstrap"` | prompt text | only consulted when the host has no built-in text for that kind. |

### Detection & status

| Export | Called with | Returns | When missing |
|---|---|---|---|
| `detectFromTitle(title)` | terminal title | bool | `false`. |
| `detectFromOutput(text)` | output chunk | bool | `false`. |
| `screenHasTui(screenText)` | screen text | bool | `false`. |
| `supportsUnboundSubmitRecovery()` | — | bool | `false`. Opt in only when this provider can safely recover submits without an authenticated session; each key still requires the provider classifier and live composer evidence. |
| `extractSessionName(screen)` | screen text | string or `null` | `null`. |
| `classifyTabState(snapshot)` | `{tabId, agentType, screenText, …}` (TabStateSnapshot) | TabStateClassification or `null` | native fallback; else `null` (generic classifier serves the tab). |
| `isOutputNoise(text)` | output chunk | bool | `false`. |
| `hasEventMarkers(chunk)` | output chunk | bool | built-in heuristic (`❯`, "proceed", "Esc to cancel", "Tab to amend", numbered options). |
| `detectEvents(text)` | output chunk | string[] of event names | universal interactive-prompt detector. **Only `"permission_request"` is mapped**; every other returned name is dropped until the core event enum grows. |

### Interactive prompts

| Export | Called with | Returns | When missing |
|---|---|---|---|
| `updatePromptSafeChoice(prompt)` | parsed prompt | one uniquely declared safe option value or `null` | native fallback; else `null`. Core uses the option-response API after ManualUpdate classification; never choose the apply-update option. |
| `updatePromptAvailability(screen)` | classified ManualUpdate screen | `{currentVersion, latestVersion}` or `null` | one unambiguous structural three-component version pair (`->` or `→`); otherwise `null`. The target supplies the durable provider/version update key. |
| `parsePrompt(text)` | screen text | `{question, options: [[id, label], …]}` or `null` | universal permission-prompt parser. |
| `buildPromptResponse(optionIndex, screen)` | selected index, live screen | array of **base64-encoded** byte strings, one per PTY write step (vim j/k, arrows, escape sequences — whatever the TUI expects; `screen` lets you navigate relative to the live highlight) | arrow-down × index + Enter. |
| `directoryTrustPromptResponse(screen)` | live clean screen text | `[key]` (exactly one base64 key toward trusting the folder), `[]` to hold a recognized trust dialog for the user, or `null` when none is shown. Core writes one key per observed frame only when `isDirectoryTrusted(cwd)` confirms the exact current folder. | `null` — directory-trust dialogs stay with the user. |
| `isDirectoryTrusted(cwd)` | current tab working directory | bool — true only when the provider's shared configuration explicitly trusts this exact folder; core fails closed on false or errors. | `false` — automatic trust responses stay disabled. |
| `launchOnboardingResponse(screen)` | live clean screen text | `{frameKey, step}` for the next declared safe key (`step` is base64), or `null` | `null` — owner-facing prompts stay with the owner. Core checks delegated Worker/Planner screen changes off the PTY output path, deduplicates each focus frame, and re-observes after each key. |
| `launchOnboardingSafeChoice(screen)` | the same live screen | exact provider-parsed safe option value, or `null` | `null` — model-bound onboarding is unsupported without a declared option. |
| `permissionPromptResponse(toolName, prompt, screen)` | hook-reported tool name, PendingPrompt, screen | `{optionIndex, rule, steps}` (base64 steps) or `null`. Only for a single-choice tool approval; return `null` for questions, plan decisions, trust gates and anything without an explicit decline option. | `null` — per-tab auto-confirm never answers this provider's prompts. |
| `permissionHookCapability()` | — | `{protocol, options: [{label, value}]}` or `null`. `claude-permission-request` requires distinct `allow` and `deny` options, a declared hook roster entry and explicit per-provider `permission_hook_decisions = true`. This declares response data, not automatic answering authority. | `null` — retain the interactive permission flow. |
| `parseContextUsage(record)` | Provider-owned statusline or transcript record | `{used_tokens, window_tokens, cost_usd, source}` or `null`; numeric counts, USD or null, bounded source identifier. Never include raw payload or identifiers. Zero window means unknown, not a guessed denominator. Host ingestion additionally requires the per-provider `context_feed` boolean and exact session/nonce binding. | `null` — no context observation. |

### Session discovery

| Export | Called with | Returns | When missing |
|---|---|---|---|
| `sessionFilePath(cwd, sessionId)` | cwd, id | path string or `null` | manifest: `sessions.rootDir` + encoded cwd + `<id>.<fileExtension>`. |
| `sessionExists(cwd, sessionId)` | cwd, id | bool or `null` (unknown/unreadable) | existence check on `sessionFilePath`. |
| `sessionUsageSources(sessionFilePath, sessionId)` | the host-resolved session file, id | paths of further usage logs owned by that exact session (e.g. sub-agent transcripts beside it); each is ingested into the KPI ledger with its own cursor | native fallback; else only the session file is ingested. |
| `usageSources(cwd, sessionId)` | cwd, id | `[{path, sessionId?}]` — every usage log this provider owns for the session. `sessionId` scopes a log shared by many sessions (e.g. one sqlite database) so a sibling's rows are never charged to this tab; a per-session file omits it. | the session file plus `sessionUsageSources`; else no log is ingested. |
| `detectSessionId(cwd, exclude, maxAgeMs)` | cwd, excluded ids, recency budget (default 300 000) | id or `null` | manifest-driven scan of the sessions dir for a **single** uuid-ish candidate (stem ≥ 32 chars containing `-`) modified within the budget; any ambiguity ⇒ `null` (honest over guessing). |
| `detectLaunchSession(evidence)` | `{cwd, launchMarker, launchedAtMs, processes: [{pid, depth, startToken}]}` for this tab's own launch; `processes` is its PTY tree with OS start times | `{sessionId, source: "pid_registry" \| "launch_marker"}` or `null` | `null`. Core asks only after a hooked provider missed its launch binding deadline, then binds through the same ownership validation as a hook report. Read only records keyed to this launch (a pid whose recorded start equals `startToken`, or text carrying `launchMarker`); never cwd or recency. |
| `detectActiveSession(cwd, currentSid)` | cwd, current id | id or `null` | `null`. |
| `detectSessionModel(cwd, sessionId)` | cwd, id | model id or `null` (resolved through the model catalog) | `null`. |
| `sessionModelTranscriptBoundary(cwd, sessionId)` | cwd, id | `{lastEntryUuid, lastEntryTimestamp, lastEntryType, boundaryFound}` or `null` — cursor for the exact session transcript at relaunch; a UUID-less user/assistant cursor uses its type and timestamp | `null` records an unknown cursor; `restart_resume` proceeds and leaves model confirmation pending. |
| `detectSessionModelAfterBoundary(cwd, sessionId, boundary)` | cwd, id, cursor | model id from an assistant entry after a known cursor or `null` | `null`; unknown cursors and old transcript entries never confirm a switch. |
| `detectSessionReasoningEffort(cwd, sessionId)` | cwd, id | effort id or `null` | `null`. |
| `sessionCreatedMs(cwd, sessionId)` | cwd, id | epoch ms or `null` | `null`. |
| `enumerateSessions()` | — | `[{sessionId, provider?, projectPath?, title?, summary?, filePath, createdTs?, modifiedTs?}]` | manifest-driven walk of `rootDir` (one directory level, uuid-ish stems), calling `describeSession` per file. |
| `findSession(sessionId)` | id | `{sessionId, provider?, projectPath?, title?, summary?, filePath, createdTs?, modifiedTs?}` or `null` — the provider's own record for exactly that id; must not scan history or guess by cwd/recency | manifest-driven `<rootDir>/*/<id>.<ext>` probe (one stat per project dir) plus one `describeSession`; else the bundled native fallback; else not found. Backs `GET /api/sessions/{id}`. |
| `describeSession(filePath, head)` | file path, first ~64 lines (≤64 KB) | `{title?, summary?, projectPath?}` | only ever called **from** the fallback enumerator; skipping it just leaves those fields empty. |

### Chat UI

| Export | Called with | Returns | When missing |
|---|---|---|---|
| `parseSessionMessages(chunk)` | line-aligned JSONL chunk (≤4 MB per call; the host chunks the file) | `[{role, text, ts?, isMeta?}]` | native fallback; else `[]`. Exporting it declares the `session_file` content source for `sessions search`. |
| `readSessionContent(sessionId, {maxBytes, hintTerms})` | id, the newest-bytes budget, ASCII-lowercased terms a returned message must hold | `{messages: [{role, text, ts?, isMeta?}], truncated}` or `null` when the store is unreadable | native fallback; else the provider has no shared-store content. For providers whose sessions share one store (`filePath` is the store): exporting it declares the `shared_store` source, so search reads each session through it and never opens the store as a file. Read only that session, scan at most `maxBytes` newest bytes, and set `truncated` when older content went unscanned. |
| `sessionJsonlPath(cwd, sessionId)` | cwd (pre-canonicalized when `cwdEncoding` is `dash_encoded`), id | path or `null` | native fallback. Exporting this is one of the signals that flips `supports_chat_ui` on. |
| `parseSessionDelta(chunk)` | line-aligned chunk read by the host tailer (host owns all offset math) | `{messages: RichMessage[]}`; each row sets `recordIndex`, the chunk line it came from, which the host maps to the record start byte that names uuid-less rows (`rec-{start}-{row}`) | the window is skipped with a warning and the offset still advances — the tailer never retry-loops. |
| `structuredToolFileDiffs(part)` | raw structured tool part | `FileDiff[]` or `null`; map only confirmed successful edits. The host uses this for live, recovered and history rows. | native fallback; else `null`. |
| `usesStructuredChat()` | — | bool | native fallback; else `false`. |
| `readStructuredChat(cwd, sessionId, cursor)` | cwd, id, opaque cursor | StructuredChat | native fallback; else empty. |
| `parseInteractiveEvents(msg)` | RichMessage | InteractiveElement[] | called only when `spawn.interactiveEvents` is true **and** the export exists; otherwise native fallback / `[]`. |

### Usage, models, schemas

| Export | Called with | Returns | When missing |
|---|---|---|---|
| `parseUsage(rawText, nowMs)` | captured usage screen, epoch ms | UsageSnapshot-shaped object (snake_case fields) or `null` | provider has no usage parser; the poller skips it (pairs with `spawn.usageProbeCommand`). |
| `fetchUsage(nowMs)` | epoch ms | raw usage payload string or `null` | usage stays probe-based. **Exporting this flips the provider to fetch-based usage** (no TUI probe). |
| `discoverModels()` | — | `[{id, displayName, description?, available?}]` or `null` | `null` = honest "no live discovery"; the manifest `models` seed stands. A `null` return never falls back to the seed either — it means "nothing new learned". |
| `modelGroupUsage()` | — | `[{group, messages, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, cost_micros, spend_cents?, spend_limit_cents?, spend_remaining_cents?, spend_resets_at_ms?}]` (snake_case; `cost_micros` is USD × 1e6, spend fields are cents) | `[]` — the picker shows no per-group usage. `group` matches a manifest `modelGroups` id (or a bare model-id prefix); core renders it for the selected model's group. The optional `spend_*` fields drive the tab's spend capsule: an absent limit stays null (honest), never a fabricated value, and exporting `modelGroupUsage()` also earns the auto-injected Usage Tracking toggle. |
| `settingsSchema()` | — | SettingField[] | manifest-derived settings sections. Evaluated **once** and cached — keep it a pure constant. |
| `notificationsSchema()` | — | SettingField[] | `[]`. Cached with `settingsSchema`. |
| `providerSelfUpdateCapability()` | — | capability object or `null` | native fallback; else `null`. |

### Rule & integration

`installBridgeRule`, `uninstallBridgeRule`, `installIntegration`,
`uninstallIntegration`, `restoreIntegration` all return `{ok: bool, error?}`.

| Export | Called with | When missing |
|---|---|---|
| `installBridgeRule(payload)` | rule markdown | payload is written to manifest `rule.path` (dirs created). |
| `uninstallBridgeRule()` | — | `rule.path` file removed if present. |
| `checkIntegration()` | — | `{status: "ok"}`. A declared-but-invalid `hookEventRoster` short-circuits to an error before the export is consulted. |
| `installIntegration(apiPort, options)` | port, `{consent, …}` | no-op success. `options.consent` must be true or the host refuses before calling anything. |
| `uninstallIntegration()` | — | no-op success. |
| `restoreIntegration()` | — | error: "provider integration has no recoverable backup". |
| `grantIntegrationTrust(managedKeys)` | managed hook keys from `integration.trust_grant` | `{granted: false, degraded: true, reason}`. |

### Caveats — declared but not bridged

Transcribed limits a JS-only author must know:

- **`structured_pending_prompt` has no JS bridge.** It delegates only to a
  native fallback — a JS-only provider that sets
  `spawn.supportsStructuredPrompts` declares a capability it can never serve.
- **`installSkills` / `uninstallSkills` are never called on JS.** `skills.dir`
  is parsed and re-exported, but skill delivery runs through trait defaults
  (no-ops) or the native provider.
- **`checkRuleStatus` / `installRule` and `readUsage`** are likewise
  native/trait-default only.
- **`detectEvents`** maps `"permission_request"`, `"turn_error"`,
  `"task_complete"`, and `"turn_started"`; other event names are silently
  dropped.

## The stop key

The host sends `commands.stopKeys` in order, or the single `commands.stopKey`
when no sequence is declared. `stopKeyWaitForOutput` inserts an event-driven
screen-change wait between keys. `supports_stop` follows whether a valid key
sequence exists. Provider choice and timing stay in the manifest.

## Fixture capture — the codex pattern

Status parsing, tab-state classification, and usage parsing are tested against
**captured real TUI screens**, not synthesized strings:

1. Run the real CLI in a Domios tab and drive it to the state you need (a
   permission prompt, the `/usage` screen, a model picker, an auth wall).
2. Capture the visible screen text — `codeterm tab get-text --tab <ID>` — and
   save it verbatim under `tests/fixtures/` (subdirectories per concern:
   `tab-state/`, usage snapshots, pickers). Redact secrets; keep layout intact,
   because your parser sees exactly these bytes in production.
3. Write a TS test that loads the **built** `plugin.js` (not the TS source) in
   a `node:vm` context with a stub `host`, and asserts the export's output for
   each fixture — see `plugins/provider-codex/tests/classify-tab-state.test.ts`
   for the loader shape.
4. Run with `npx tsx <test-file>` — Node only, no Rust toolchain.

When the vendor's TUI changes, re-capture the fixture rather than editing it by
hand: fixtures are evidence, and vendors change copy without notice (which is
also why detection uses structural fingerprints, never literal strings).

## What tests do you owe? (the parity story)

The honest seam, in three parts:

- **JS-only (community) providers owe TS tests against the built `plugin.js`**,
  using captured fixtures as above, runnable with `npx tsx` alone. No Rust
  toolchain is required or expected. For in-repo plugins the suite is
  registered by hand in `scripts/plugin-tests.sh`; an external repo runs its
  own tests.
- **The Rust parity suite (`js_runtime/parity_test.rs` + per-provider modules)
  remains the in-repo gate for bundled drop-ins** — providers with a native
  counterpart, where every trait method a JS drop-in implements must agree with
  the native path on shared fixtures. Its contract lists enumerate the bundled
  ids; a third-party provider is deliberately outside it.
- **Promotion is the boundary.** The moment a community plugin is promoted into
  the main repo as a bundled drop-in is the moment maintainer-side parity cases
  get written — by the maintainers, with the Rust toolchain. Until then, no
  Rust-side obligation exists.

No machinery bridges these tiers (no Rust-free parity runner, no WASM QuickJS
harness); the promotion-time boundary is the cheap, honest seam.

## Internal references

Moved out of the embedded `codeterm docs` pages because they describe Domios's own source:

- [prompt-detection.md](prompt-detection.md) — provider prompt detection architecture
- [references/session-storage.md](references/session-storage.md) — provider session storage and search scanning
- [references/secret-backends.md](references/secret-backends.md) — native secret backends and the retired credential erase

Session manifests may declare `sessions.rootEnvironment: {variable, subdir}` for read-only inherited account homes. Empty/unset falls back to `rootDir`; the selected directory still requires sandbox read and environment permission. This does not grant write access or install integrations in that home. Credentials may declare `fileEnvironment` with the same shape; both default and selected credential paths remain denied to plugin reads.

For a model-free control handshake, declare `spawn.capabilityProbe: "stream_json_initialize"`. The host owns process containment, isolation, decoding and bounded project/binary cache; no new sandbox host function is exposed. This capability uses `capability_probe_enabled`, default false, and explicit `POST /api/tabs/{id}/agent/capabilities/refresh`. Native and JS providers must agree on the declaration. Supported initialize data feeds cached slash suggestions, model aliases and subscription/mode display. Never send a user turn or usage request from a capability probe.

`host.commandTranscriptStdout(recordJson, declarationJson)` exposes the shared
pure Rust command-record parser to JS providers. It returns only declared
stdout from caller-supplied JSON (1 MiB record and 24 KiB declaration bounds),
with no filesystem, network, environment or subprocess access. The guard,
confirmation reader and native/JS model-label detection share that parser;
shared command fixtures cover native/QuickJS parity.

`agentRuntime.usage: true` declares live usage publication and earns the shared Usage Tracking setting. Runtime auth events declare `billingMode`; account rate windows reach the subscription meter and exact-thread token events reach the session chip. `TokenUsage.cost_micros` is optional reported USD cost, never an estimate. These pure data extensions grant no host filesystem, environment, network or subprocess access.
