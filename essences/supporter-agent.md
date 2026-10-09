# Supporter Agent: the OS spinal cord

The Supporter Agent gives Domios small, fast judgments where agents would otherwise need a person watching every tab. It ranks memory and names tab states. The intended model-first policy makes the connected model the first opinion at event-level classification points, with screen and text heuristics as fallback. The deterministic core owns the resulting reflexes: keeping unattended work moving, recovering the General Agent within hard limits and coordinating [provider updates](core.md#provider-lifecycle). The Supporter supplies classification to those responsibilities. The [vision](vision.md) states why this belongs in the OS; the [capability map](capabilities.md) distinguishes rollout from planned coverage.

This is a shared host capability, not a Team member or a task executor. **The model names the state; deterministic evidence authorizes the action.** It has no input, permission, session or task authority of its own.

## Connection and fallback

A saved connection is configured, not proven reachable. Connections keep each adapter's own model, endpoint and write-only token. Saved membership includes the connection table, the active adapter and adapters with stored tokens. One optional active adapter runs requests when its declared token requirements are satisfied; absence means stopped. Switching keeps the previous connection and its token, model changes never activate, and removing active never selects a fallback adapter. Usage history survives removal. Connection enables all Supporter uses automatically, with no feature switches. Always on means ready to respond to lifecycle, output and request events, not continuous polling. Calls have bounded failure deadlines; no delay or elapsed interval proves readiness.

Without a connected adapter, Domios behaves exactly as before: ordinary recall, tab behavior and deterministic recovery remain available. An unavailable model, failed call or exhausted explicit request cap leaves the deterministic fallback available. Automatic plan reminders need a connected Choice adapter's validated `action_needed` approval at confidence of at least 0.80 for an actionable recipient and its observation. A validated `blocked` verdict sends no reminder to that recipient and creates one host-authored owner notice per distinct blocker; the notice offers wait, pause plan and decide, and proposes a pause when the blocker stops every actionable task. Absence, failure, an exhausted cap, denial or uncertainty sends neither reminder nor blocker notice, and neither elapsed time nor hard events substitute for approval. After acceptance, a reminder is eligible again only when its recipient's task status, assignment, report or owner message changes; unrelated plan changes do not reset the backoff. The approval only says whether a host-authored reminder is useful; it never chooses the recipient, text or session, and it never gates reports, owner messages, decisions, failure alerts or recovery. An incomplete set of memory judgments falls back as one unit; the [memory contract](memory.md) owns result preservation and access accounting. A per-search diagnostic override is not a feature switch.

Adapters declare choice, yes/no probability (`noul`), score, token and usage capabilities. Core never selects behavior by adapter or provider identity. The contract targets a class of decision models, not a particular product. TypeSafe Jev is the current reference model; OpenCode exposes configured local and remote models through the same capability boundary. An adapter's availability does not certify its judgment quality.

## Evidence and model-first classification

The planned model-first policy applies at event-level decisions: launch, deadlines, idle or busy transitions, blocking prompts, worker and cron launch, and exit. With a connected adapter, a label other than `other` at confidence of at least 0.60 supplies the classification. Screen and text heuristics run only as fallback: no adapter, timeout, error, exhaustion of an explicit cap, `other` or lower confidence. On disagreement, a usable model verdict replaces the heuristic classification; deterministic evidence still decides whether any action is permitted. The policy follows declared decision capabilities, independent of the selected model or supplier.

Per-byte, per-frame and per-delivery hot paths remain deterministic and never wait on a model. Independently authorized deterministic recovery does not wait for classification either. Model-first classification replaces a guess, never proof of process ownership, exit or session binding, and never invents an option a provider did not declare.

A model label cannot establish session ownership, readiness or delivery; it answers a permission only by choosing a provider-declared option under the [prompt-answer rule](#provider-declared-prompt-answers). Input delivery never parses the screen to refuse text or keys: neither a heuristic nor a model may turn transcript text resembling a trust, permission or update dialog into a delivery refusal. The [PTY contract](core.md#core:pty) remains authoritative for payload-once submission and exact delivery evidence.

Tab-state observations belong to the tab's current launch. Model-assisted tab recovery actions require two consecutive agreeing strong observations of that launch plus the action's own deterministic evidence. Values below 0.60 are ignored; model-driven owner notifications require two agreeing observations each at least 0.80; automatic General Agent recovery actions require at least 0.90. An `other` verdict is journaled but produces neither model action nor hint; the event-level heuristic fallback may still classify the state under its existing authority. A first strong observation may produce a non-actionable hint.

Automatic model-assisted restart and recovery actions also require measured qualification: at least 0.95 precision in the automatic-action confidence band on independently captured, human-labelled held-out states, plus a successful live recovery oracle for the adapter, model and criteria in use. Without that evidence the model-assisted restart or recovery remains notify-only. Provider-declared prompt answers are not gated by this qualification. This safety qualification is not a user feature switch.

## Provider-declared prompt answers

An interactive prompt that an agent meets at launch or during a cron run, and that offers provider-declared options for folder trust, tool permission or a provider update, may be answered automatically. One confident model answer is enough: a choice of one provider-declared option at confidence of at least 0.80. When no adapter is connected, the call fails or is capped, or the answer is `other` or less confident, the structural heuristic decides as fallback, and a prompt it cannot resolve needs owner attention. The answer is always one of the provider's declared options, sent through the existing prompt-response surface; the model never types invented text. Login, authentication, unavailable-model and quota prompts stay with the owner.

## Bounded recovery and unattended work

The protected Main General Agent's recovery is driven by lifecycle events. Quiescing, launch grace, process ownership and restart limits are checked at the event. Independent General-session exits cannot restart Main or consume its recovery budget. Model observations cannot restart a live provider merely because its screen looks broken.

| Condition | Authority and permitted outcome |
|---|---|
| Dead PTY or confirmed provider exit to a surviving shell | Deterministic lifecycle evidence authorizes recovery; the surviving-shell case also requires a fresh process-tree check that the provider is absent. A model observation is audit-only. Resume a verified owned session, or use the General Agent's bounded fresh-launch recovery when none is available. |
| Startup deadline exceeded or error loop | Qualified, agreeing model observations and deterministic evidence that the PTY is dead or the provider is absent permit one restart for that condition. If that restart does not resolve the error, notify the owner rather than repeatedly restarting for the same condition. |
| Provider update prompt | Answered under the [prompt-answer rule](#provider-declared-prompt-answers); while an update runs, only the provider's declared safe option is chosen under the [provider lifecycle](core.md#provider-lifecycle). |
| Login, authentication, unavailable model or quota | Notify the owner with choices; never answer automatically. |
| Directory trust | At launch or during a cron run, answered under the [prompt-answer rule](#provider-declared-prompt-answers); otherwise the [folder-trust contract](permissions.md#folder-trust-and-provenance) decides, and uncovered trust needs owner attention. |
| Command or tool permission | At launch or during a cron run, answered under the [prompt-answer rule](#provider-declared-prompt-answers); otherwise the [agent execution-access contract](permissions.md#agent-execution-access) and the tab's permission policy decide. |

At most two automatic General Agent restarts are permitted per recovery episode, or fewer if configured. Exhaustion produces a failure notification and latches owner-required state; only explicit owner action permits another episode. Every automatic recovery action is journaled and produces a verbose notice. The deterministic crash path retains these limits even when no adapter is connected.

Hidden memory workers and crons use the [folder provenance contract](permissions.md#folder-trust-and-provenance). They otherwise receive the same evidence-based safe-prompt and notification handling as other unattended agents. Being hidden never removes a blocker from view of its responsible agent or owner, grants permission or converts an incomplete run into success.

Trust, permission and login Supporter notices are suppressed for tabs launched directly by the user without agent or Team ownership; their journal evidence remains. The General Agent, agent-owned tabs, hidden workers and crons retain notices. Restored tabs with the same condition produce one grouped notification per state at restore completion. Durable deduplication prevents the same tab, state and launch from notifying again after a restart; a new state or launch may notify again.

## Budget, fan-out and spend

The intended default has no request cap; this default change is planned for 1.12.4. All Supporter requests are still counted and journaled across tabs, including hidden workers, crons and Blueprints. A request counts once even when it contains several named questions. Only an explicitly configured positive cap refuses requests on count; zero or an absent cap means unlimited. At an explicit cap, recall uses baseline results, event-level classification uses its heuristic fallback, and status shows when capacity becomes available. Cap transitions are journaled once per transition. Visible estimated cost and the out-of-credit alert provide the default cost guard.

Questions over one shared state belong in one decision request: tab label, prompt visibility and any safe-choice question share the same screen and context. Memory candidates can likewise share a query and candidate pool. Fan-out is adopted only when measured answer quality is no worse than the previous path; fewer calls alone do not justify lower relevance. Until that comparison qualifies shared-state memory scoring, the per-candidate path remains available, counted and subject to any explicit cap. Every answer remains subject to its own confidence and authority rules, and memory fallback stays all-or-nothing.

Spend is estimated from response usage and adapter-declared prices. Missing usage or pricing remains unknown, never zero. Domios keeps no owner-entered or inferred balance. An out-of-credit response (HTTP 402) alerts once and short-circuits further calls with deterministic fallback. Status distinguishes credit exhaustion from an explicit request cap.

## Blueprints

[Blueprint graphs](sentinels.md) use the Supporter at their decision points. A changed observation receives one choice over the options its author offered, with `other` always available, through the shared request accounting; an unchanged observation makes no call. A model choice acts only at confidence of at least 0.80, once per transition, and selects an offered option without constructing a command, target or response. The [Blueprint contract](sentinels.md) owns graph shape, authority, escalation, improvement and lifetime.

## Data and visibility

Memory judgments send the query, candidate titles and bounded body text. Tab judgments send recent screen text and runtime context to the selected adapter. The Guide explains these inputs before the owner connects it.

Credentials belong to the instance and adapter. They enter through CLI stdin or the write-only Settings field, never argv, logs or status responses. Owner-only local files (0600 on Unix and owner access controls on Windows) keep them outside the vault to meet recall latency requirements. Status reports token presence and permission evidence without returning the token.

A bounded durable journal records automatic actions, budget transitions and decision distributions, including question counts, the adapter-reported model, criteria version and input hash. Tokens and raw input never enter it. The Settings > Agents > Supporter Agent card shows only saved connections, requests per hour, requests per day and estimated cost, without feature toggles; activity and verdicts belong to the Mission Control feed. Failed status reads retain visibly stale rows and disable mutations; an unknown outcome is refreshed before retry. Token-save success with activation failure shows saved, not active and offers retry activation.

Planned visibility adds a spend row for the selected priced decision adapter beside provider allowances: estimated spend today and this month. Unpriced or local adapters show request counts without an invented monetary amount. The spinal-cord activity feed lives in Mission Control, machine-wide for General and scoped to each Team, never in Settings. It shows scrubbed, length-capped questions and answers built from daemon-owned query, memory and tab titles, declared bucket names and action metadata. A model request with several named questions appears as one row that expands to every question and answer; confidence, budget, fallback and operation codes remain in its details. Raw screen text, probe output, credentials and request bodies never appear in the feed. Team attribution belongs to the recorded event, so it remains correct after a tab disappears. Journal events refresh these views; the views neither poll nor change decisions. Confidence has readable text, not color alone.

## Future inbox triage

A future direction is to use the spinal cord to triage incoming work for LLM agents, saving their tokens by routing routine inputs before they need a full reasoning turn. This is not a planned feature yet: the input contract, categories and handoff behavior remain undecided. It grants no new execution authority and makes no delivery or token-saving claim today.

## Onboarding, the Guide and the web

After first-run setup is complete, the General Agent introduces the Supporter once and links to its Guide section. Connection remains the owner's choice. The Guide explains the spinal-cord idea in plain language first: what it helps with, what it sends, fallback without an adapter, budget and spend, and the limits of recovery. Connection instructions and technical detail follow that explanation.

Future onboarding and the web client build on this same host-owned judgment and evidence contract. The marketing website consumes these essences to explain the product; it is not a control surface or a source of new runtime authority. A new presentation may expose state, explain a choice or request an authorized operation; it cannot create a separate permission policy, model authority or recovery loop. Detailed future onboarding and web-client journeys remain roadmap work.

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
