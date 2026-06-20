# Facet 3: State Taxonomy + Fingerprint Catalog

Scope: PTY agent panes only. Core provides snapshots, lifecycle edges, action
execution, and UI surfacing. Provider plugins own fingerprints, extraction,
navigation, reset parsing, and confidence.

## Enum Proposal

```ts
type AgentPaneState =
  | { kind: "working_streaming"; phase?: "thinking" | "tool" | "assistant_text" | "unknown"; partial?: AssistantPartial }
  | { kind: "turn_complete"; promptReady: true }
  | { kind: "clarifying_question"; question: string; options?: PromptOption[]; defaultOptionId?: string }
  | { kind: "presented_options"; prompt: string; options: PromptOption[]; defaultOptionId?: string; intent: OptionIntent }
  | { kind: "waiting_on_auth"; method?: "oauth" | "api_key" | "token" | "workspace_trust" | "unknown" }
  | { kind: "rate_limited"; resetAt?: string; timezone?: string; options?: PromptOption[]; defaultOptionId?: string }
  | { kind: "update_prompt"; currentVersion?: string; nextVersion?: string; options: PromptOption[]; defaultOptionId?: string }
  | { kind: "error_crash"; category: "missing_binary" | "auth_failed" | "network" | "provider_crash" | "unknown"; message?: string }
  | { kind: "idle_stuck"; reason: "turn_owed_no_motion" | "empty_screen" | "unknown"; lastMotionMs?: number }
  | { kind: "unknown" };

type PromptOption = {
  id: string;
  label: string;
  row?: number;
  col?: number;
  selected?: boolean;
  destructive?: boolean;
  value?: string;
};

type OptionIntent =
  | "permission"
  | "model_choice"
  | "plan_choice"
  | "continue_or_cancel"
  | "unknown";

type AssistantPartial = {
  text: string;
  stablePrefixBytes?: number;
  isFinal?: boolean;
};
```

Current `PaneState` maps as:

| Current | Proposed |
| --- | --- |
| `Working` | `working_streaming` or `turn_complete` depending on prompt readiness |
| `WaitingOnAuth` | `waiting_on_auth` |
| `UpdatePrompt` | `update_prompt` |
| `RateLimited` | `rate_limited` |
| `ClarifyingQuestion` | `clarifying_question` when a question exists, otherwise `presented_options` |
| `IdleStuck` | `idle_stuck` |
| `Unknown` | `unknown` |

## Boundary Shape

Provider input should stay small and provider-local:

```ts
type PaneStateSnapshot = {
  paneId: string;
  agentType: string;
  sessionId?: string | null;
  cwd?: string | null;
  screenText: string;       // vt100 rendered tail, current AS1 source
  screenRows?: ScreenRow[]; // future: row/col/cell attrs for cursor/menu geometry
  outputTail?: string;      // recent raw text tail for crashes and launch errors
  status: "working" | "idle" | "waiting" | "dead";
  turnOwed?: boolean;
  lastMotionMs?: number;
};

type PaneStateClassification = {
  state: AgentPaneState;
  confidence: number;
  evidence: Evidence[];
  action?: PaneStateAction;
};

type Evidence = {
  id: string;
  weight: number;
  matched: boolean;
  rows?: number[];
};
```

Rules:

| Rule | Owner |
| --- | --- |
| Only classify from the pane's own session/screen | core snapshot builder |
| Provider-specific strings, menus, reset clocks, and navigation | provider plugin |
| Global confidence threshold, auth no-auto-act guard, action execution | core reactor |
| Fast fixture corpus and negative corpus | provider package |
| Streaming partial extraction | provider plugin, using the same snapshot plus output deltas |

## Universal Shapes

These are cross-provider shape classes. Each provider still implements its own
matcher and weights.

| Shape | Structural signals | Extract |
| --- | --- | --- |
| Busy TUI | spinner/progress glyph, interrupt footer, tool-running row, changing content hash | `working_streaming.phase`, partial text |
| Prompt-ready | stable prompt/input bar, no busy footer, cursor in input row, no pending selection modal | `turn_complete` |
| Vertical numbered menu | `selector? + number + delimiter + label` on 2+ adjacent rows, one selected row | options, selected/default |
| Horizontal option row | labels separated by wide gaps inside a box/footer | options, selected/default if highlighted |
| Boxed modal | border rows, title/header row, body, option row/footer | prompt, options, category |
| Auth gate | login/API-key/token/workspace-trust prompt shape, often menu or boxed modal | method |
| Rate limit gate | limit header + reset clock/timezone + wait/upgrade choices | `resetAt`, `timezone`, options |
| Update gate | version transition + release notes/update choices | versions, options |
| Crash/error | process exit text, stack/error block, missing command failure, dead status | category, message |
| Idle stuck | turn is owed, status parked, stable screen hash longer than timeout, no known modal | reason |

## Provider Fingerprint Catalog

Legend:

| Mark | Meaning |
| --- | --- |
| Shipped | implemented or fixture-backed now |
| Captured | repo has a non-state TUI fixture usable as seed |
| Needed | needs live capture before implementing |
| Plugin-local | never goes into Rust core |

### Working / Streaming

| Provider | Signals | Variations | Status |
| --- | --- | --- | --- |
| Claude | Busy footer with interrupt hint; thinking/working glyph at line start; tool rows updating; changing content hash outside status bar | `esc` footer, `Ctrl+C` footer, `Working...` suffix, tool list visible while prose contains question-like text | Shipped as busy veto in Claude classifier |
| Codex | Header/model block plus active line prefixed by progress glyph; cancel footer; prose ending in "working" marker; changing content hash | `ctrl+c to cancel`, triangular progress marker, tool rows with bullets | Shipped as negative fixture seed, not classifier state |
| Cursor | Banner/input UI with mode row and prompt bar; expected busy state should show running tool/progress indicator in Cursor Agent TUI | Current repo fixture only has banner and trust gate | Needed |
| OpenCode | TUI/status fragments such as tab/commands footer; block/box drawing; expected busy state should show active tool/status panel | Runtime detector knows TUI fragments, not state | Needed |

Output: `working_streaming`; no action. It is a hard veto for menus seen inside
streaming prose.

### Turn Complete / Done Idle

| Provider | Signals | Variations | Status |
| --- | --- | --- | --- |
| Claude | Stable input prompt/footer, no interrupt footer, no modal/menu, cursor in prompt row | Empty prompt after assistant turn; shell-like prompt if agent exited to shell should be `error_crash` or `unknown` | Needed |
| Codex | Stable Codex input area, no cancel footer, no modal/update/auth menu, cursor in prompt row | Fresh pane after update skip; resumed session prompt | Needed |
| Cursor | Prompt bar visible, mode row stable, no trust/modal/running marker | `Auto`/mode row variants | Needed |
| OpenCode | TUI footer/input line stable, no permission box/running marker | `tab agents`, `ctrl+p commands` footer variants | Needed |

Output: `turn_complete`; no key action. This should be separated from
`working_streaming` so "finished vs not" does not depend on text like "wait".

### Clarifying Question

| Provider | Signals | Variations | Status |
| --- | --- | --- | --- |
| Claude | Question line ending in `?` + vertical numbered menu + confirm/cancel footer | Selected row may use `>`, `❯`, `▶`, `➤`; question may be above menu; default is selected row or first row | Shipped |
| Codex | Question line + vertical numbered menu; must be parked, not cancel-footer streaming | Existing negative fixture proves menu+question inside "Working..." must not fire | Needed |
| Cursor | Expected Cursor modal/question uses prompt body plus option controls; exact row shape not captured | Could overlap with workspace trust; trust is auth, not question | Needed |
| OpenCode | Existing `parsePrompt` handles permission question, but generic clarification shape is not captured | Boxed body + horizontal options may be permission, not clarification | Needed |

Output: `clarifying_question`; surface question/options; never auto-answer.

### Presented Options

| Provider | Signals | Variations | Status |
| --- | --- | --- | --- |
| Claude | Vertical numbered menu with selected row and footer; no auth/rate/update signature | Plan/model/tool choices; labels may include long descriptions | Partly shipped through clarify/rate menu parser |
| Codex | Vertical numbered menu; update/auth classifiers take precedence | Model or approval menus need capture | Needed |
| Cursor | Workspace trust option row currently `▶ [a] Trust this workspace`; non-auth options need capture | Hotkey in brackets may be option id; selected with arrow | Captured for auth only |
| OpenCode | Permission box title + question line + horizontal option row split by 2+ spaces | `Allow once`, `Allow always`, `Reject`; labels inside box after stripping borders | Captured in parser, no state capability yet |

Output: `presented_options`; action depends on `intent`. Permission prompts
surface or follow explicit auto-approve policy; core must not infer provider id.

### Waiting On Auth

| Provider | Signals | Variations | Status |
| --- | --- | --- | --- |
| Claude | Login title + method menu; API-key option; confirm/exit footer | OAuth account vs API key; selected row defaults first | Shipped |
| Codex | Sign-in title + OpenAI/ChatGPT/API-key menu or browser auth instruction | Browser-open prompt; API key path; selected first row | Shipped |
| Cursor | Workspace Trust Required box; body says code/file access; trust option with hotkey | Trust gate is auth/consent, not a normal menu; skipPermissions pre-write bypasses only by explicit user choice | Captured |
| OpenCode | Provider login/config failure screen; exact TUI auth prompt not captured | May appear as config error, missing token, or external provider auth failure | Needed |

Output: `waiting_on_auth`; surface only. No keys, even with high confidence.

### Rate Limited

| Provider | Signals | Variations | Status |
| --- | --- | --- | --- |
| Claude | Limit header + reset clock + optional timezone + wait/upgrade menu + confirm/cancel footer | Reset missing; selected row may be upgrade; wait option not always default; timezone in parentheses | Shipped |
| Codex | Rate/usage-limit screen with reset/wait/retry shape, if CLI renders one | Need distinguish prose mentioning limits from parked prompt | Needed |
| Cursor | Subscription/quota modal or provider API limit message | Exact TUI unknown | Needed |
| OpenCode | Router/provider rate-limit message or model quota box | Could be plain error text from backend, not TUI menu | Needed |

Output: `rate_limited`; parse `resetAt` and `timezone` when present. If reset
parse is weak, surface instead of scheduling.

### Update / Upgrade Prompt

| Provider | Signals | Variations | Status |
| --- | --- | --- | --- |
| Claude | CLI update prompt, if Claude Code renders one | Exact TUI unknown; must not confuse plan upgrade in rate-limit menu with CLI update | Needed |
| Codex | Version transition + release notes + update/skip vertical menu + continue footer | ASCII `->` or Unicode arrow; `Skip` can be option 2 or 3; default is update | Shipped |
| Cursor | Cursor Agent update notification, if shown inside TUI | Exact TUI unknown; installer prompts must be auth/consent or update based on shape | Needed |
| OpenCode | OpenCode update/outdated prompt, if shown inside TUI | Exact TUI unknown | Needed |

Output: `update_prompt`; plugin chooses safe navigation such as skip/remind
later. Never default-enter an update.

### Error / Crash

| Provider | Signals | Variations | Status |
| --- | --- | --- | --- |
| Claude | Dead status plus launch/runtime error tail; auth failures reclassify as auth | Missing binary, nonzero exit, uncaught stack, network error | Needed |
| Codex | Dead status plus launch/runtime error tail; auth failures reclassify as auth | Missing binary from real launch error, not PATH pre-check; panic/stack | Needed |
| Cursor | `cursor-agent` not found; trust/auth failures; sqlite/session corruption | Workspace trust is `waiting_on_auth`, not crash | Needed |
| OpenCode | Missing binary; config/plugin error; provider/router error; process exit | Existing integration checks can support config categories | Needed |

Output: `error_crash`; surface. Missing CLI install flow is a separate
confirm-and-relaunch capability, not an automatic state action.

### Should Be Working But Stuck

| Provider | Signals | Variations | Status |
| --- | --- | --- | --- |
| Claude | Turn owed + stable non-modal screen + no busy footer longer than provider timeout | Last visible text may say "wait" or contain a stale menu-like transcript; classifier must require lifecycle evidence | Needed |
| Codex | Same lifecycle shape; no cancel footer; no update/auth/rate modal | Prevent false positives from normal shell prompt after command completes | Needed |
| Cursor | Same lifecycle shape; no trust gate; prompt bar stable after a submitted task | Needs prompt-submission ledger | Needed |
| OpenCode | Same lifecycle shape; no permission box/running panel | Needs prompt-submission ledger | Needed |

Output: `idle_stuck`; surface or nudge only through policy. This is not a text
fingerprint; it is lifecycle state plus absence of known parked prompts.

## Priority / Precedence

Provider classifiers should apply precedence before scoring lower-risk states:

1. `waiting_on_auth`
2. `error_crash` when process is dead or launch failed
3. `working_streaming` busy veto
4. `rate_limited`
5. `update_prompt`
6. `clarifying_question`
7. `presented_options`
8. `turn_complete`
9. `idle_stuck`
10. `unknown`

Rationale: auth and dead states prevent unsafe actions; busy veto prevents
question/menu false positives while output is still moving.

## Fast Test Approach

Per provider package:

| Test set | Fixtures | Assertions |
| --- | --- | --- |
| Positive states | one file per real captured screen | exact `kind`, confidence >= threshold, extracted payload, exact action |
| Variations | selected row changes, CRLF, arrow glyphs, missing reset clock, reordered menu | same state, adjusted extraction/navigation |
| Negatives | normal working prose, diffs, shell output, tool lists, prose mentioning limits/update/auth | no action; `working_streaming` or `unknown` only |
| Precedence | auth+menu, busy+question, rate+upgrade, dead+auth | highest-priority state wins |
| Fuzz-lite | mutate one literal per fixture, keep layout | structural match survives one copy change |
| Parity | native/QuickJS against shared fixtures | identical state/action/data |

Fixture roots should be normalized to:

```text
plugins/<provider>/tests/fixtures/pane-state/
  positive/<state>/<case>.txt
  negative/<case>.txt
```

Current roots can be migrated mechanically from:

```text
plugins/provider-claude/tests/fixtures/pane-state/
plugins/provider-codex/tests/fixtures/pane-state/
```

## Capture Gaps

| Provider | Must capture before implementation |
| --- | --- |
| Claude | turn-complete prompt, CLI update prompt, crash/missing-binary, generic presented-options |
| Codex | turn-complete prompt, clarifying question parked without busy footer, rate-limit, crash/missing-binary |
| Cursor | busy, turn-complete, auth/login besides workspace trust, clarification/options, rate-limit, update, crash |
| OpenCode | busy, turn-complete, auth, clarification beyond permission, rate-limit, update, crash |

## Risks

| Risk | Mitigation |
| --- | --- |
| Text like "wait" looks parked | require lifecycle status + menu/modal geometry + busy veto |
| Finished vs still streaming is ambiguous | add `turn_complete` and `working_streaming` as distinct states |
| Provider copy changes | score structural layout, not exact vendor prose; mutation tests |
| Wrong key press | plugin emits navigation only after target option row is identified |
| Reset time wrong | parse in provider; include timezone evidence; surface when weak |
| Core grows provider branches | CI grep/review: no provider ids in reactor decisions |

<!-- revision: 1 -->
