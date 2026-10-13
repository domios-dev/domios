# Codex launch option inventory

This inventory describes source behavior, not a claim that the runtime is activated. The live manifest remains inactive pending the Orchestrator's switch-over instruction.

## Presets and sources

`provider-codex/plugin.json` declares no `modelProviderConfig`, no preset catalogue and no `readModelPresets`, `applyModelPresets` or `modelPresetLaunchArgs` hooks. `src/models.ts` discovers model names and reasoning levels only. The native `AgentProvider::model_provider_config` default is `None`; the JS implementation also requires manifest opt-in. Consequently `service/model_providers/presets.rs:117-126` does not append preset flags for the current Codex provider: its preceding adapter lookup returns early. There are zero bundled Codex model-provider presets to translate.

User CLI profiles and caller-supplied launch args are different from those absent model-provider presets. The PTY `buildLaunchCommand` and `buildResumeCommandWithContext` forward arbitrary caller args with shell quoting. Saved `restorable_launch.args` can retain them. The runtime accepts neither opaque argv nor named CLI profiles; it refuses those visibly before executing hooks. A pure plugin cannot read profile files, recover omitted launch args, or bypass that host refusal.

| Source / option | PTY behavior | Runtime mapping / status |
| --- | --- | --- |
| Manifest `modelLaunchArgsFormat`: `-m MODEL` | Selects model | `RuntimeSettings.model` -> start/resume `model`, turn `model` |
| Manifest effort format: `-c model_reasoning_effort="EFFORT"` | Selects reasoning effort | `RuntimeSettings.effort` -> start/resume config, turn `effort` |
| Authorized skip permissions / autoApproveFlags | Adds bypass-approvals-and-sandbox and bypass-hook-trust | A12 host-authorized `mode: full-access` -> approval `never`, sandbox `danger-full-access` / policy `dangerFullAccess`; hook trust has no fabricated equivalent |
| Manifest `additionalWritableDirFlag`: `--add-dir` | Allows writes outside cwd | A12 host-validated `additionalDirectories` -> start/resume `config["sandbox_workspace_write.writable_roots"]`, workspace-write turn policy `writableRoots` |
| Host launch sandbox git metadata / agent pad directories | Appends add-dir for granted worktree metadata and pad | Same A12 directory handoff; host decides which directories are granted |
| Manifest Linux sandbox network args | `-c sandbox_workspace_write.network_access=true` when the host applies its network policy | No A12 network field: host must supply its intended network policy explicitly through an approved typed handoff; activation decision needed if sandboxed Linux still requires this override |
| `CODEX_LAUNCH_CONFIG` | Disables startup update check, raises project instruction budget to 65536 bytes | Start/resume config contains `check_for_update_on_startup:false`, `project_doc_max_bytes:65536` |
| Task / system prompt | Unix developer_instructions config; Windows generated per-tab profile; optional initial prompt | `openThread.instructions` -> `developerInstructions`; initial user input remains ordinary submitTurn |
| Unix / Windows resume helpers | Exact session id, `resume`, `--no-daemon`; Windows owned profile for task/trust | Exact thread/resume; direct owned stdio process replaces daemon selection; no profile-file artifacts |
| Caller `--sandbox` / `--ask-for-approval` | Independent execution sandbox and approval policy | No argv accepted. No independent policy handoff approved; raw flags refused. Four authorized modes express their defined policy pairs |
| `--approve-for-me` | Automatic approval reviewer in workspace-write | `RuntimeSettings.mode: auto` -> reviewer `auto_review`, approval `on-request`, workspaceWrite |
| Older `--full-auto` spelling | Not advertised by installed 0.160 help; historically on-request + workspace-write | Corresponding behavior is `mode: auto-accept-edits`; raw legacy flag stays refused |
| Caller `--search` | Enables live web search | RPC config `web_search: live` can express it, but no launch setting handoff approved; raw flag refused |
| Caller `-c` / `--config`, `--enable`, `--disable` | TOML dotted overrides / feature booleans | RPC thread config supports parsed JSON overrides, but no arbitrary launch-config handoff approved; raw flags refused |
| Caller `--oss` / `--local-provider` / model_provider config | Selects local/custom provider; may involve CLI model download/startup interaction | RPC `modelProvider` can select a configured provider, but no typed launch handoff approved. Native Codex config still supplies its default; raw flags refused |
| Caller `-p` / `--profile` | Layers `$CODEX_HOME/<name>.config.toml` over base config | Refused: no thread RPC selector for a general CLI config profile. `permissions` is a different permission profile and must not be substituted |
| Caller `-C` / `--cd` | Changes working root | Host-composed `RuntimeContext.cwd`; raw flag stays refused |
| Caller `-i` / `--image` | Initial user image | Ordinary TurnInput attachments, mapped to image/localImage; raw flag stays refused |
| Caller TUI `--no-alt-screen` | Terminal presentation | No Chat equivalent; raw flag refused |
| Caller `--remote`, auth-token-env, --no-daemon | Changes TUI app-server connection | Runtime process/credential ownership stays host-owned; raw flags refused |
| Caller `--worktree` | Creates managed worktree | No implicit worktree creation; host must provide an already-authorized cwd |
| Caller arbitrary subcommands / other argv | Runs arbitrary supported CLI operation | Refused; not a runtime operation |

Source references: `plugins/provider-codex/plugin.json` spawn/commands; `src/plugin.ts` CODEX_LAUNCH_CONFIG, buildLaunchCommand, buildResumeCommand, buildResumeCommandWithContext; `src/models.ts`; `src-tauri/src/service/model_providers/presets.rs`; `service/orchestrator/launch/command.rs` and its sandbox grant helpers (read only); installed Codex 0.160.0 `--help`. No user config, secrets or account profiles were read for this inventory.

## Structured runtime settings

The plugin consumes the A12 RuntimeSettings fields only: model, effort, host-authorized mode, and host-validated additionalDirectories. ProviderSettings cannot override the authorized sandbox/approval mode or inject arbitrary launch config/argv. No new profile/argv parser or permission bypass is introduced.

Start/resume config merges the existing provider launch defaults, host directories, then RuntimeSettings effort. Additional directories are effective only in workspace-write; full access already permits them and read-only remains read-only. Hook code never authorizes a requested bypass: the host supplies the already-authorized mode per A12.

T3 counterpart: `CodexAdapterV2.ts` codexRuntimeModeTurnDefaults and codexThreadRuntimeParams. The approach uses typed policy fields and per-thread config overrides above native Codex config. Deliberate adaptation: our launch defaults preserve the existing Domios instruction budget; our A12 directories and authorization come from the host's frozen RuntimeSettings contract. We do not resolve files or spawn a second process from pure hooks.

## Decisions still owned by the host / Orchestrator

A12 makes model, effort, authorized mode and directories structurally available; host launch/restore integration must supply them before activation. General CLI profiles and raw args remain visibly refused by decision. There is no claim of backward-compatible arbitrary argv launches. Hook-trust bypass is not implied by model execution permission: the owner must decide whether native Codex hooks remain enabled/trusted on runtime sessions. Linux launch network policy also needs explicit host-to-provider-settings handoff if it must differ from configured Codex defaults.
