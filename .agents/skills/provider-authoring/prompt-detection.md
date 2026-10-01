# Prompt detection

How provider prompts reach tab status and how `tab respond` answers the active choice.

## Live path

1. PTY output updates the per-tab terminal screen held by `ScreenState`.
2. `process_pty_output()` checks that visible screen when an event, active prompt, auto-approve setting, or changed-screen trust candidate requires it.
3. Structured prompt providers contribute their pending prompt from the structured-prompt store. The live output path does not infer prompts from arbitrary provider screen text. Folder trust is a narrow screen-classified case: the dedicated `parseDirectoryTrustPrompt` provider capability runs after a native check for an active two-choice menu, its footer, directory context, and trust-related content. The Codex and Claude general `parsePrompt` parsers are not used on this live path.
4. `PromptDetector` stores the resulting question and options as `pending_prompt` on the tab and emits the updated mux state.
5. `codeterm tab status` shows the question and one-based options. `codeterm tab status --json` includes the `pending_prompt` object.
6. `codeterm tab respond <option> --tab <id>` rechecks that the prompt is still active on screen, asks the provider for response keys, and writes nothing if the prompt has gone away. This is the owner answer path for Claude and Codex folder-trust prompts, including Team pair tabs. Prompt detection does not create notifications; an agent that needs interrupting owner attention must explicitly run `codeterm notify send "<title>" --level action_needed`.

Directory trust auto-response is separate from prompt display. It requires CodeTerm-owned directory provenance before answering. A trust dialog without that authority can still be surfaced through the dedicated prompt parser, so an Orchestrator can inspect the numbered choices and respond through the tab command.

## Classifier boundaries

The native trust precheck checks menu structure and screen position rather than question wording. The Codex trust classifier requires a directory path context, a folder/directory/workspace trust or load question, a description of local policy material, exactly two numbered Yes/No choices, one active selection, and an active footer. Claude requires its titled trust question, exactly two focused Yes/No trust choices, and the active footer. Bypass-style warnings, generic approvals, model pickers, and numbered answer lists do not qualify as folder trust.

The providers' general parsers remain separate from the live trust parser. This prevents a numbered list in an agent answer from becoming a false active prompt. Provider parsing runs only after the screen changed and the native trust precheck matched.

## Sources

| File | Role |
|------|------|
| `src-tauri/src/agent/prompt.rs` | Prompt state machine and decisions |
| `src-tauri/src/agent/detector.rs` | Native structural prompt detection |
| `src-tauri/src/service/output.rs` | Changed-screen gating and live prompt application |
| `src-tauri/src/service/directory_trust.rs` | Trust provenance, cheap candidate check, and guarded response |
| `plugins/provider-codex/src/prompt.ts`, `plugins/provider-claude/src/folderTrust.ts` | Dedicated trust classifiers and response mappings |
| `src-tauri/src/service/tab.rs` | On-screen recheck and `tab respond` handling |

## Verification surfaces

Coverage includes generic numbered choices, active Codex trust choices, trust-dialog negative fixtures, stale-screen refusal, native/JavaScript parity, and the response path. Canonical QA should run the focused Rust and plugin tests from the task report after refreshing the generated Codex plugin bundle.
