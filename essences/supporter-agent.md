# Supporter Agent: the OS spinal cord

The Supporter Agent gives CodeTerm small, fast judgments where agents would otherwise need a person watching every tab. It ranks memory, names tab states and supplies a second opinion on uncertain screen and text heuristics. The deterministic core owns the resulting reflexes: keeping unattended work moving, recovering the General Agent within hard limits and coordinating provider updates. The [vision](vision.md) states why this belongs in the OS; the [capability map](capabilities.md) distinguishes rollout from planned coverage.

This is a shared host capability, not a Team member or a task executor. **The model names the state; deterministic evidence authorizes the action.** It has no input, permission, session or task authority of its own.

## Connection and fallback

A selected, registered adapter is connected when its declared token requirements are satisfied. Connection enables all Supporter uses automatically, with no feature switches. Always on means ready to respond to lifecycle, output and request events, not continuous polling. Calls have bounded failure deadlines; no delay or elapsed interval proves readiness.

Without a connected adapter, CodeTerm behaves exactly as before: ordinary recall, tab behavior and deterministic recovery remain available. An unavailable model, failed call or exhausted budget leaves the deterministic path unchanged. An incomplete set of memory judgments falls back as one unit; the [memory contract](core.md#core:memory) owns result preservation and access accounting. A per-search diagnostic override is not a feature switch.

Adapters declare choice, yes/no probability (`noul`), score, token and usage capabilities. Core never selects behavior by adapter or provider identity. TypeSafe Jev and a local adapter such as LM Studio serve the same host contract; an adapter's availability does not certify its judgment quality.

## Evidence and second opinions

Every screen or text heuristic belongs within the second-opinion coverage boundary. A reliable deterministic signal takes precedence; an uncertain heuristic or one with known misfires receives a bounded confirm/veto judgment through the shared Supporter. Confirmation adds no authority. A veto suppresses only the heuristic's proposed action, never an independently authorized deterministic recovery. Without a usable judgment, the existing deterministic behavior remains unchanged.

A model label cannot establish session ownership, readiness, permission or delivery. Input delivery never parses the screen to refuse text or keys: neither a heuristic nor a model may turn transcript text resembling a trust, permission or update dialog into a delivery refusal. The [PTY contract](core.md#core:pty) remains authoritative for payload-once submission and exact delivery evidence.

Observations belong to the tab's current launch. Model-assisted automatic actions require two consecutive agreeing strong observations of that launch plus the action's own deterministic evidence. Values below 0.60 are ignored; model-driven owner notifications require two agreeing observations each at least 0.80; automatic General Agent actions require at least 0.90. An `other` verdict is journaled but produces neither action nor hint. A first strong observation may produce a non-actionable hint.

Automatic model-assisted recovery also requires measured qualification: at least 0.95 precision in the automatic-action confidence band on independently captured, human-labelled held-out states, plus a successful live recovery oracle for the adapter, model and criteria in use. Without that evidence the model-assisted action remains notify-only. This safety qualification is not a user feature switch.

## Bounded recovery and unattended work

The General Agent's recovery is driven by lifecycle events. Quiescing, launch grace, process ownership and restart limits are checked at the event. Model observations cannot restart a live provider merely because its screen looks broken.

| Condition | Authority and permitted outcome |
|---|---|
| Dead PTY or confirmed provider exit to a surviving shell | Deterministic lifecycle evidence authorizes recovery; the surviving-shell case also requires a fresh process-tree check that the provider is absent. A model observation is audit-only. Resume a verified owned session, or use the General Agent's bounded fresh-launch recovery when none is available. |
| Startup deadline exceeded or error loop | Qualified, agreeing model observations and deterministic evidence that the PTY is dead or the provider is absent permit one restart for that condition. If that restart does not resolve the error, notify the owner rather than repeatedly restarting for the same condition. |
| Provider update prompt | The prompt must be classified as an update prompt and the provider must declare its safe option. A model-assisted answer additionally requires agreement on that exact option at at least 0.90 confidence. Answer through the existing prompt-response surface; never type an invented answer. Missing or disagreeing evidence requires owner attention. |
| Login, authentication, unavailable model or quota | Notify the owner with choices; never answer automatically. |
| Directory trust | The existing deterministic trust policy decides: provenance, shared trust or a live same-provider tab with a verified session in the exact canonical directory may cover it. Use the provider-declared trust option; uncovered trust needs owner attention. |
| Command or tool permission | The tab's own permission policy decides. The model never grants approval. |

At most two automatic General Agent restarts are permitted per recovery episode, or fewer if configured. Exhaustion produces a failure notification and latches owner-required state; only explicit owner action permits another episode. Every automatic recovery action is journaled and produces a verbose notice. The deterministic crash path retains these limits even when no adapter is connected.

Hidden workers and crons receive the same evidence-based trust, safe-prompt and notification handling as other unattended agents. Being hidden never removes a blocker from view of its responsible agent or owner, grants permission or converts an incomplete run into success.

Trust, permission and login Supporter notices are suppressed for tabs launched directly by the user without agent or Team ownership; their journal evidence remains. The General Agent, agent-owned tabs, hidden workers and crons retain notices. Restored tabs with the same condition produce one grouped notification per state at restore completion. Durable deduplication prevents the same tab, state and launch from notifying again after a restart; a new state or launch may notify again.

## Budget, fan-out and spend

All Supporter uses share one rolling request budget across tabs, including hidden workers and crons. The default is 600 requests per hour. A request counts once even when it contains several named questions. Exhaustion makes no further model calls: recall uses baseline results, observation skips, and status shows when capacity becomes available. Budget transitions are journaled once per transition.

Questions over one shared state belong in one decision request: tab label, prompt visibility and any safe-choice question share the same screen and context. Memory candidates can likewise share a query and candidate pool. Fan-out is adopted only when measured answer quality is no worse than the previous path; fewer calls alone do not justify lower relevance. Until that comparison qualifies shared-state memory scoring, the per-candidate path remains available and budgeted. Every answer remains subject to its own confidence and authority rules, and memory fallback stays all-or-nothing.

Spend is estimated from response usage and adapter-declared prices. Missing usage or pricing remains unknown, never zero or an invented balance. An optional owner-entered balance gives an estimated remaining amount, not an authoritative provider balance. Crossing below an estimated $1 remaining alerts once; an out-of-credit response alerts once and short-circuits further calls with deterministic fallback. Status distinguishes credit exhaustion from the request budget.

## Data and visibility

Memory judgments send the query, candidate titles and bounded body text. Tab judgments send recent screen text and runtime context to the selected adapter. The Guide explains these inputs before the owner connects it.

Credentials belong to the instance and adapter. They enter through CLI stdin or the write-only Settings field, never argv, logs or status responses. Owner-only local files (0600 on Unix and owner access controls on Windows) keep them outside the vault to meet recall latency requirements. Status reports token presence and permission evidence without returning the token.

A bounded durable journal records automatic actions, budget transitions and decision distributions, including question counts, the adapter-reported model, criteria version and input hash. Tokens and raw input never enter it. Settings > Agents > Supporter Agent exposes connection, activity, recent verdicts, request counts, budget and estimated spend without feature toggles.

## Provider updates

The planned coordinator extends the existing provider update responsibility under this evidence boundary. Within the connected Supporter behavior, a declared update check or classified update prompt initiates one automatic update per provider and target version, deduplicated across tabs and restarts. The no-adapter baseline remains unchanged. It uses the provider's declared update operation; only production participates by default, with explicit opt-in for another instance.

While an update runs, prompts receive only the provider-declared safe option, such as Skip; a working tab is never told to choose Update now. Success restarts idle tabs through session-aware recovery, including the General Agent. A turn in flight or an in-progress task keeps a tab pending until its next eligible idle event. Each tab restarts at most once per update. Failure restarts nothing and leaves the old version running. One summary reports restarted and pending tabs, is updated as pending work finishes, and retains failure evidence; update and restart actions are journaled.

## Onboarding, the Guide and the web

After first-run setup is complete, the General Agent introduces the Supporter once and links to its Guide section. Connection remains the owner's choice. The Guide explains the spinal-cord idea in plain language first: what it helps with, what it sends, fallback without an adapter, budget and spend, and the limits of recovery. Connection instructions and technical detail follow that explanation.

Future onboarding and the future web experience build on this same host-owned judgment and evidence contract. A new presentation may expose state, explain a choice or request an authorized operation; it cannot create a separate permission policy, model authority or recovery loop. Their detailed journeys remain roadmap work.

```map
anchors:
  - core:supporter-agent
  - core:general-agent
  - capability:supporter-agent
  - capability:memory-decision-rerank
  - capability:provider-updates
  - capability:guided-onboarding
  - capability:web-ui
```
