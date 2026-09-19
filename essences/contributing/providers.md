# Authoring an Agent Provider Plugin

How to build a JS drop-in agent provider for CodeTerm — the manifest schema, the
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
| `installer` | map platform→string | Per-OS install command (`macos` / `linux` / `windows`). |
| `updater` | map platform→string | Per-OS update command launched in a visible shell from an update notification. |
| `models` | `[{id, displayName, description?, available?}]` | Model seed registered with the model catalog. Each entry carries `id`, `displayName`, optional `description`, optional `available`; `available: false` ships an alias disabled until live discovery confirms it. |
| `modelGroups` | `[{id, label}]` | Provider-owned groupings of `models` for the picker (e.g. the upstream company a routed model belongs to). `id` matches a model-id prefix, `label` is the text the picker renders; core never maps a vendor id to a label. Empty ⇒ one flat list. |
| `credentials` | `[{id, file, public?, secret?}]` | Credential files the host reads on the plugin's behalf. Declaring one also revokes the plugin's own read access to that file. A declared file's fingerprint is captured with the cached model snapshot, so connecting a provider (the file appearing or changing) re-probes its models without a rebuild. |
| `defaultModel` | string? | Default model id. |
| `reasoningEfforts` | `[{id, displayName, description?}]` | Reasoning-effort levels. |
| `defaultReasoningEffort` | string? | Default effort id. |
| `spawn` | object | Spawn semantics — table below. Defaults apply when the whole block is absent. |
| `sessions` | object? | Session-file layout — table below. |
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
| `verifySubmit` | `{"kind":"shell_prompt","timeoutSecs":N}` \| `{"kind":"tui_input_drained","initialKeyDelayMs":N}` \| `{"kind":"none"}` | `shell_prompt` / 5 s | How the host verifies a submitted message actually went in. `initialKeyDelayMs` defaults to 0. |
| `busySubmit` | `{"kind":"submit_normally"}` \| `{"kind":"key","key":"<bytes>"}` | `submit_normally` | What to send when submitting while the agent is generating. |
| `sessionIdInjection` | `"none"` \| `"fresh"` \| `"fresh_and_fork"` | `none` | Whether the host generates and injects a session id at launch (and on fork). |
| `statusBarRows` | u16 | 0 | TUI status-bar rows to exclude from output scraping. |
| `agentStateStatusReconciliation` | bool | false | Provider participates in agent-state ↔ status reconciliation. |
| `supportsLazySkills` | bool | false | Provider can lazy-load skills via trigger words; otherwise skills merge into the system prompt up-front. |
| `hasHooks` | bool | false | Provider has hook-based detection. |
| `hookEventRoster` | `[{event, route}]`? | absent | Declared hook events: each entry names an `event` and a core ingestion `route`. Unknown routes fail the **whole roster** closed. Absent ⇒ native fallback's roster, else SessionStart-only when `hasHooks`. |
| `interactiveEvents` | bool | false | Plugin exports `parseInteractiveEvents`; host calls it to extract interactive prompts. False ⇒ native fallback handles it. |
| `supportsStructuredPrompts` | bool | false | Provider exposes a structured prompt source; host builds `pending_prompt` from it instead of scraping the PTY. **JS-only providers cannot serve this** — see caveats. |
| `structuredPromptServer` | bool | false | The structured handle is a local server the provider must bind; host allocates `127.0.0.1:port` and passes it as `structuredPromptServer` at launch. Pull-handle providers leave this false. |
| `usageProbeCommand` | string? | absent | TUI command that prints the usage screen (e.g. `/usage`, `/status`). Pairs with the `parseUsage` export; the usage poller skips providers without both. |

### `sessions`

Drives the manifest fallbacks for session discovery (`sessionFilePath`,
`sessionExists`, `detectSessionId`, `enumerateSessions`, `findSession`).

| Field | Type / legal values | Default | Meaning |
|---|---|---|---|
| `rootDir` | string (required) | — | Filesystem root the provider writes session files into. `~/` expands to the home dir. E.g. `~/.claude/projects`. |
| `cwdEncoding` | `"dash_encoded"` \| `"none"` | `dash_encoded` | How a cwd maps to a subdirectory name: `dash_encoded` turns `/Users/foo/code` into `-Users-foo-code`; `none` uses the path as-is. |
| `fileExtension` | string | `"jsonl"` | Session file extension. |

### `commands`

All fields optional.

| Field | Type | Meaning |
|---|---|---|
| `compact` | string? | In-agent compact command (e.g. `/compact`). |
| `freshContext` | string? | In-agent fresh-context command. |
| `exit` | string? | In-agent clean-exit command. |
| `stopKey` | string? | Raw key bytes the host writes into the PTY to **interrupt** the current generation (not a clean exit). All four bundled providers declare Esc (`"\u001b"`) — see "The stop key" below. Absent ⇒ the UI hides the Stop affordance (`supports_stop` is derived from this field). |
| `modelSwitchFormat` | string? | In-agent model-switch template with `{model}` placeholder (e.g. `/model {model}`). |
| `autoApproveFlags` | string[] | Launch flags for permission-bypass mode (e.g. `--yolo`, `--force`). Empty ⇒ the UI hides the bypass choice (`supports_permission_bypass` is derived). |
| `modelLaunchArgsFormat` | string[] | Launch-arg template, each element may contain `{model}`. Empty ⇒ default `--model <id>`. |
| `reasoningEffortLaunchArgsFormat` | string[] | Launch-arg template with `{effort}`. Absent ⇒ provider has no scriptable effort flag even if it lists effort levels. |

### `integration`

| Field | Type / legal values | Meaning |
|---|---|---|
| `kind` | `"claude_settings_json"` \| `"provider_managed"` \| `"none"` | `claude_settings_json`: edit a JSON file's `hooks` object with curl-back commands. `provider_managed`: provider JS owns a reviewable config merge + backup/restore. `none`: PTY parser does it all. |
| `settingsPath` | string | Where the integration is stored (e.g. `~/.claude/settings.json`). |
| `trust_grant` (alias `trustGrant`) | object? | One field, `managedKeys` (string[]): managed hook keys core passes to `grantIntegrationTrust` per install. |

### `permissions`

Provider-relevant subset (full contract in [PLUGINS.md](../PLUGINS.md)):

| Field | Type | Meaning |
|---|---|---|
| `transportEvents` | string[] | Transport event streams the plugin may subscribe to. |
| `sendText` | bool | May write text into tabs. |
| `network` | object | `host.fetch` host allowlist: an `allow` array of hosts. |
| `subprocess` | object | `host.exec` binary allowlist: an `allow` array of binaries. |
| `secrets` | bool | May use the per-plugin secret bucket. |
| `agent_commands` (alias `agentCommands`) | bool | Spawned agents may route `codeterm plugin <id> <verb>` to this plugin's `onAgentCommand`. |
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
| `extractSessionName(screen)` | screen text | string or `null` | `null`. |
| `classifyTabState(snapshot)` | `{tabId, agentType, screenText, …}` (TabStateSnapshot) | TabStateClassification or `null` | native fallback; else `null` (generic classifier serves the tab). |
| `isOutputNoise(text)` | output chunk | bool | `false`. |
| `hasEventMarkers(chunk)` | output chunk | bool | built-in heuristic (`❯`, "proceed", "Esc to cancel", "Tab to amend", numbered options). |
| `detectEvents(text)` | output chunk | string[] of event names | universal interactive-prompt detector. **Only `"permission_request"` is mapped**; every other returned name is dropped until the core event enum grows. |

### Interactive prompts

| Export | Called with | Returns | When missing |
|---|---|---|---|
| `parsePrompt(text)` | screen text | `{question, options: [[id, label], …]}` or `null` | universal permission-prompt parser. |
| `buildPromptResponse(optionIndex, screen)` | selected index, live screen | array of **base64-encoded** byte strings, one per PTY write step (vim j/k, arrows, escape sequences — whatever the TUI expects; `screen` lets you navigate relative to the live highlight) | arrow-down × index + Enter. |
| `directoryTrustPromptResponse(screen)` | live clean screen text | `[key]` (exactly one base64 key toward trusting the folder), `[]` to hold a recognized trust dialog for the user, or `null` when none is shown. Core asks only for an agent tab in the directory CodeTerm created whose launch session is not yet authenticated, and writes one key per observed frame. | `null` — directory-trust dialogs stay with the user. |
| `permissionPromptResponse(toolName, prompt, screen)` | hook-reported tool name, PendingPrompt, screen | `{optionIndex, rule, steps}` (base64 steps) or `null`. Only for a single-choice tool approval; return `null` for questions, plan decisions, trust gates and anything without an explicit decline option. | `null` — per-tab auto-confirm never answers this provider's prompts. |

### Session discovery

| Export | Called with | Returns | When missing |
|---|---|---|---|
| `sessionFilePath(cwd, sessionId)` | cwd, id | path string or `null` | manifest: `sessions.rootDir` + encoded cwd + `<id>.<fileExtension>`. |
| `sessionExists(cwd, sessionId)` | cwd, id | bool | existence check on `sessionFilePath`. |
| `sessionUsageSources(sessionFilePath, sessionId)` | the host-resolved session file, id | paths of further usage logs owned by that exact session (e.g. sub-agent transcripts beside it); each is ingested into the KPI ledger with its own cursor | native fallback; else only the session file is ingested. |
| `usageSources(cwd, sessionId)` | cwd, id | `[{path, sessionId?}]` — every usage log this provider owns for the session. `sessionId` scopes a log shared by many sessions (e.g. one sqlite database) so a sibling's rows are never charged to this tab; a per-session file omits it. | the session file plus `sessionUsageSources`; else no log is ingested. |
| `detectSessionId(cwd, exclude, maxAgeMs)` | cwd, excluded ids, recency budget (default 300 000) | id or `null` | manifest-driven scan of the sessions dir for a **single** uuid-ish candidate (stem ≥ 32 chars containing `-`) modified within the budget; any ambiguity ⇒ `null` (honest over guessing). |
| `detectLaunchSession(evidence)` | `{cwd, launchMarker, launchedAtMs, processes: [{pid, depth, startToken}]}` for this tab's own launch; `processes` is its PTY tree with OS start times | `{sessionId, source: "pid_registry" \| "launch_marker"}` or `null` | `null`. Core asks only after a hooked provider missed its launch binding deadline, then binds through the same ownership validation as a hook report. Read only records keyed to this launch (a pid whose recorded start equals `startToken`, or text carrying `launchMarker`); never cwd or recency. |
| `detectActiveSession(cwd, currentSid)` | cwd, current id | id or `null` | `null`. |
| `detectSessionModel(cwd, sessionId)` | cwd, id | model id or `null` (resolved through the model catalog) | `null`. |
| `detectSessionReasoningEffort(cwd, sessionId)` | cwd, id | effort id or `null` | `null`. |
| `sessionCreatedMs(cwd, sessionId)` | cwd, id | epoch ms or `null` | `null`. |
| `enumerateSessions()` | — | `[{sessionId, provider?, projectPath?, title?, summary?, filePath, createdTs?, modifiedTs?}]` | manifest-driven walk of `rootDir` (one directory level, uuid-ish stems), calling `describeSession` per file. |
| `findSession(sessionId)` | id | `{sessionId, provider?, projectPath?, title?, summary?, filePath, createdTs?, modifiedTs?}` or `null` — the provider's own record for exactly that id; must not scan history or guess by cwd/recency | manifest-driven `<rootDir>/*/<id>.<ext>` probe (one stat per project dir) plus one `describeSession`; else the bundled native fallback; else not found. Backs `GET /api/sessions/{id}`. |
| `describeSession(filePath, head)` | file path, first ~64 lines (≤64 KB) | `{title?, summary?, projectPath?}` | only ever called **from** the fallback enumerator; skipping it just leaves those fields empty. |

### Chat UI

| Export | Called with | Returns | When missing |
|---|---|---|---|
| `parseSessionMessages(chunk)` | line-aligned JSONL chunk (≤4 MB per call; the host chunks the file) | `[{role, text, ts?, isMeta?}]` | native fallback; else `[]`. |
| `sessionJsonlPath(cwd, sessionId)` | cwd (pre-canonicalized when `cwdEncoding` is `dash_encoded`), id | path or `null` | native fallback. Exporting this is one of the signals that flips `supports_chat_ui` on. |
| `parseSessionDelta(chunk)` | line-aligned chunk read by the host tailer (host owns all offset math) | `{messages: RichMessage[]}`; each row sets `recordIndex`, the chunk line it came from, which the host maps to the record start byte that names uuid-less rows (`rec-{start}-{row}`) | the window is skipped with a warning and the offset still advances — the tailer never retry-loops. |
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
- **`detectEvents`** maps only `"permission_request"`; other event names are
  silently dropped.

## The stop key

What CodeTerm sends to interrupt a generating agent is exactly
`commands.stopKey` — `JsProvider::stop_keystroke()` returns the manifest value
verbatim, and the descriptor's `supports_stop` is derived from its presence
(two reads of one capability, guarded by
`parity_test.rs::supports_stop_matches_stop_keystroke`). All four bundled
provider manifests — claude, codex, opencode, **and cursor** — declare Esc
(`"\u001b"`). Cursor has no native Rust provider, so its manifest is the sole
operative source: no code path sends Ctrl+C to any bundled provider. The
`AgentProvider` trait doc and the CLI help state the mechanism — the
provider's declared stopKey — not a per-provider key table.

## Fixture capture — the codex pattern

Status parsing, tab-state classification, and usage parsing are tested against
**captured real TUI screens**, not synthesized strings:

1. Run the real CLI in a CodeTerm tab and drive it to the state you need (a
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

