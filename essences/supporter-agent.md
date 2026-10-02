# Supporter Agent: the OS spinal cord

The Supporter Agent gives CodeTerm small, fast judgments where agents would otherwise need a person watching every tab. It ranks memory and names tab states. The intended model-first policy makes the connected model the first opinion at event-level classification points, with screen and text heuristics as fallback. The deterministic core owns the resulting reflexes: keeping unattended work moving, recovering the General Agent within hard limits and coordinating provider updates. The [vision](vision.md) states why this belongs in the OS; the [capability map](capabilities.md) distinguishes rollout from planned coverage.

This is a shared host capability, not a Team member or a task executor. **The model names the state; deterministic evidence authorizes the action.** It has no input, permission, session or task authority of its own.

## Connection and fallback

A selected, registered adapter is connected when its declared token requirements are satisfied. Connection enables all Supporter uses automatically, with no feature switches. Always on means ready to respond to lifecycle, output and request events, not continuous polling. Calls have bounded failure deadlines; no delay or elapsed interval proves readiness.

Without a connected adapter, CodeTerm behaves exactly as before: ordinary recall, tab behavior and deterministic recovery remain available. An unavailable model, failed call or exhausted explicit request cap leaves the deterministic fallback available. An incomplete set of memory judgments falls back as one unit; the [memory contract](core.md#core:memory) owns result preservation and access accounting. A per-search diagnostic override is not a feature switch.

Adapters declare choice, yes/no probability (`noul`), score, token and usage capabilities. Core never selects behavior by adapter or provider identity. The contract targets a class of decision models, not a particular product. TypeSafe Jev is the current reference model; future models and local adapters such as LM Studio use the same capability boundary. An adapter's availability does not certify its judgment quality.

## Evidence and model-first classification

The planned model-first policy applies at event-level decisions: launch, deadlines, idle or busy transitions, blocking prompts, worker and cron launch, and exit. With a connected adapter, a label other than `other` at confidence of at least 0.60 supplies the classification. Screen and text heuristics run only as fallback: no adapter, timeout, error, exhaustion of an explicit cap, `other` or lower confidence. On disagreement, a usable model verdict replaces the heuristic classification; deterministic evidence still decides whether any action is permitted. The policy follows declared decision capabilities, independent of the selected model or supplier.

Per-byte, per-frame and per-delivery hot paths remain deterministic and never wait on a model. Independently authorized deterministic recovery does not wait for classification either. Model-first classification replaces a guess, never proof of process ownership, exit, directory provenance, a declared safe option or session binding.

A model label cannot establish session ownership, readiness, permission or delivery. Input delivery never parses the screen to refuse text or keys: neither a heuristic nor a model may turn transcript text resembling a trust, permission or update dialog into a delivery refusal. The [PTY contract](core.md#core:pty) remains authoritative for payload-once submission and exact delivery evidence.

Tab-state observations belong to the tab's current launch. Model-assisted tab recovery actions require two consecutive agreeing strong observations of that launch plus the action's own deterministic evidence. Values below 0.60 are ignored; model-driven owner notifications require two agreeing observations each at least 0.80; automatic General Agent actions require at least 0.90. An `other` verdict is journaled but produces neither model action nor hint; the event-level heuristic fallback may still classify the state under its existing authority. A first strong observation may produce a non-actionable hint.

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

Hidden memory workers trust their daemon-owned folder, and crons trust the folder chosen by their owner when the schedule was created. Host-recorded provenance authorizes the provider-declared trust response for that exact directory; a model verdict or an arbitrary supplied path cannot grant it. This handling is already merged for 1.12.4. Hidden workers and crons otherwise receive the same evidence-based trust, safe-prompt and notification handling as other unattended agents. Being hidden never removes a blocker from view of its responsible agent or owner, grants permission or converts an incomplete run into success.

Trust, permission and login Supporter notices are suppressed for tabs launched directly by the user without agent or Team ownership; their journal evidence remains. The General Agent, agent-owned tabs, hidden workers and crons retain notices. Restored tabs with the same condition produce one grouped notification per state at restore completion. Durable deduplication prevents the same tab, state and launch from notifying again after a restart; a new state or launch may notify again.

## Budget, fan-out and spend

The intended default has no request cap; this default change is planned for 1.12.4. All Supporter requests are still counted and journaled across tabs, including hidden workers, crons and Sentinels. A request counts once even when it contains several named questions. Only an explicitly configured positive cap refuses requests on count; zero or an absent cap means unlimited. At an explicit cap, recall uses baseline results, event-level classification uses its heuristic fallback, and status shows when capacity becomes available. Cap transitions are journaled once per transition. Spend and balance alerts provide the default cost guard.

Questions over one shared state belong in one decision request: tab label, prompt visibility and any safe-choice question share the same screen and context. Memory candidates can likewise share a query and candidate pool. Fan-out is adopted only when measured answer quality is no worse than the previous path; fewer calls alone do not justify lower relevance. Until that comparison qualifies shared-state memory scoring, the per-candidate path remains available, counted and subject to any explicit cap. Every answer remains subject to its own confidence and authority rules, and memory fallback stays all-or-nothing.

Spend is estimated from response usage and adapter-declared prices. Missing usage or pricing remains unknown, never zero or an invented balance. An optional owner-entered balance gives an estimated remaining amount, not an authoritative provider balance. Crossing below an estimated $1 remaining alerts once; an out-of-credit response (HTTP 402) alerts once and short-circuits further calls with deterministic fallback. Status distinguishes credit exhaustion from an explicit request cap.

## Sentinels

Sentinels are planned for 1.12.4: cheap watchers and routers that agents create to replace LLM polling. A creator defines a deterministic probe, a classification question, a finite set of buckets and permitted routes. The probe has bounded runtime and output. Its schedule is explicitly requested work, not a sleep loop coordinating internal readiness. An unchanged output makes no model call; changed output receives one bucket classification through the shared Supporter accounting, with `other` as an exit.

Routes form a closed set: notify the creator, send a message to a named tab through the existing delivery surface, trigger a named cron, or stop. A model route requires a bucket transition at confidence of at least 0.80 and runs once for that transition. It never executes a command invented by the model. `other`, low confidence and an exhausted explicit cap produce a journal entry without routing. Without an adapter, creator-declared deterministic match rules may route; creation without either an adapter or match rules is refused.

Sentinels belong to their creator and Team, stop on their declared completion or timeout, and are removed when their Team closes. Shared agent guidance teaches every agent to use a Sentinel for watching and routing instead of spending task-model turns on repeated status checks. This contract does not grant Sentinels task, permission or delivery authority.

## Data and visibility

Memory judgments send the query, candidate titles and bounded body text. Tab judgments send recent screen text and runtime context to the selected adapter. The Guide explains these inputs before the owner connects it.

Credentials belong to the instance and adapter. They enter through CLI stdin or the write-only Settings field, never argv, logs or status responses. Owner-only local files (0600 on Unix and owner access controls on Windows) keep them outside the vault to meet recall latency requirements. Status reports token presence and permission evidence without returning the token.

A bounded durable journal records automatic actions, budget transitions and decision distributions, including question counts, the adapter-reported model, criteria version and input hash. Tokens and raw input never enter it. Settings > Agents > Supporter Agent exposes connection, activity, recent verdicts, request counts, cap state and estimated spend without feature toggles.

Planned visibility adds a spend row for the selected priced decision adapter beside provider allowances: estimated spend today and this month, and estimated remaining balance when entered by the owner. Unpriced or local adapters show request counts without an invented monetary amount. A spinal-cord activity feed is machine-wide in General and scoped to the Team inside its Mission Control surface. It shows bounded, redacted decision summaries, actions or notify-only outcomes, reasons and fallback state. Team attribution belongs to the recorded event, so it remains correct after a tab disappears. Journal events refresh these views; the views neither poll nor change decisions. Confidence has readable text, not color alone.

## Provider updates

The planned coordinator extends the existing provider update responsibility under this evidence boundary. A declared update check or classified update prompt initiates one automatic update per provider and target version, deduplicated across tabs and restarts, without a consent click. The coordinator is a deterministic host operation; model availability changes its classification path, not its authority to perform the declared update. It uses the provider's declared update operation; only production participates by default, with explicit opt-in for another instance.

While an update runs, prompts receive only the provider-declared safe option, such as Skip; a working tab is never told to choose Update now. Success restarts idle tabs through session-aware recovery, including the General Agent. A turn in flight or an in-progress task keeps a tab pending until its next eligible idle event. Each tab restarts at most once per update. Failure restarts nothing and leaves the old version running. One summary reports restarted and pending tabs, is updated as pending work finishes, and retains failure evidence; update and restart actions are journaled.

## Future inbox triage

A future direction is to use the spinal cord to triage incoming work for LLM agents, saving their tokens by routing routine inputs before they need a full reasoning turn. This is not a planned feature yet: the input contract, categories and handoff behavior remain undecided. It grants no new execution authority and makes no delivery or token-saving claim today.

## Onboarding, the Guide and the web

After first-run setup is complete, the General Agent introduces the Supporter once and links to its Guide section. Connection remains the owner's choice. The Guide explains the spinal-cord idea in plain language first: what it helps with, what it sends, fallback without an adapter, budget and spend, and the limits of recovery. Connection instructions and technical detail follow that explanation.

Future onboarding and the web client build on this same host-owned judgment and evidence contract. The marketing website consumes these essences to explain the product; it is not a control surface or a source of new runtime authority. A new presentation may expose state, explain a choice or request an authorized operation; it cannot create a separate permission policy, model authority or recovery loop. Their detailed journeys remain roadmap work.

```map
anchors:
  - core:supporter-agent
  - core:general-agent
  - capability:supporter-agent
  - capability:memory-decision-rerank
  - capability:sentinels
  - capability:provider-updates
  - capability:guided-onboarding
  - capability:web-ui
```
