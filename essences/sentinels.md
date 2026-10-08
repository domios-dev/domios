# Blueprints: self-improving action programs

A Blueprint graph is an agent's recurring action written once as a program. It runs along its own pre-planned cycle without a reasoning agent's turn, judges where judgment is needed, and hands any case it cannot handle to an explicit escalation recipient. Each case a Team member resolves can become a case the graph handles next time, so a graph improves with use. The [Supporter Agent](supporter-agent.md) supplies the judgments; the [capability map](capabilities.md) records what is in rollout and what is planned.

## Shape and cost

A graph is made of deterministic steps and decision points. A decision point offers a finite set of options, each defined in code before the graph runs. The connected decision model chooses one, and code executes exactly the chosen option. The model never constructs an option, command, target, key sequence or text, and observed output never becomes one. Without a connected adapter, author-declared match rules choose; a graph with neither is refused.

A stage may observe bounded probe output, an agent tab, or input data (inline text, a file or a URL) with an optional one-time media conversion. A `cron-run:<id-or-name>` stage waits for a run, then binds to the exact automation tab recorded for that run, including hidden tabs without Team membership. Run replacement resets the observation hash, accepted bucket, held state and per-run transition allowance while retaining transition history; run completion or tab removal unbinds the stage. A removed run tab notifies the creator, and the graph waits for another verified run.

Probes have bounded runtime and output, and their cadence is requested work, not a sleep coordinating readiness. An unchanged observation never calls the model; a changed one costs one choice in the shared Supporter accounting. A transition acts once.

## What a graph may do

A graph may act on anything its owning agent may: observe, run creator-authored commands, message tabs, trigger crons, activate stages, request a prompt answer through the existing [safe prompt-answer path](supporter-agent.md#provider-declared-prompt-answers), and stop. That path revalidates the live prompt and kind, then chooses only a provider-declared option; the graph supplies neither an option nor PTY keys. It never exceeds the owning agent's authority or its creator, Team, workspace and machine scope. Timeouts and finite transition and action budgets bound every graph; exhausting one stops the graph with a recorded reason. Each action is claimed durably before it runs and is never replayed after a restart; an uncertain outcome stays unconfirmed.

## Other, escalation and rewrite

Any decision can resolve to `other`: an unknown outcome that fits none of the offered options or falls below the confidence threshold. Reserved `other` handlers may notify or escalate to the creator or an exact same-Team agent tab. The escalation packet carries only the Blueprint and transition IDs. Adapter and budget failures never escalate.

An escalation holds that stage without another classification until the exact transition is resolved, a new run is bound, or the configured escalation timeout expires. Expiry is journaled. Each escalation cause can be delivered at most once per five minutes per Blueprint, and a durable claim is never replayed after restart. Resolution clears the observation hash and re-evaluates once.

The owning Team may finish the case and amend the graph so it becomes an offered option with its own code-defined action. Graphs are versioned: an amendment names the version it expects to replace, a stale expectation is refused, and each version keeps its author, cause and replaced definition. Any caller in the same Team and workspace may amend. The decision model never rewrites a graph.

## Budgets and request limits

A graph's transition budget defaults to 64 and has a cap of 256. Probe and direct-tab graphs spend it over their lifetime; each verified CronRun binding gets its own allowance. Accepted history is retained to a separate lifetime cap of 65,536 transitions. Budget exhaustion stops the graph with `transition_budget_exhausted` and notifies its creator once with the reason. Prompt-response requests are limited to 600 per rolling 60-second window, with at most one response request per prompt occurrence on a tab. An escalation cause can be delivered at most once per five minutes per graph.

## Ownership and lifetime

A graph belongs to its creator and its creation scope on one machine; remote execution is unavailable. A graph created before its creator joins a Team remains owned and visible to that creator, without becoming visible to other Team members. It stops on its declared completion, timeout, budget exhaustion or stop, or when its creator is removed; Team-owned graphs are removed when their Team closes. An unreadable store leaves Blueprints unavailable rather than presenting an empty healthy graph.

```map
anchors:
  - capability:sentinels
  - capability:scheduling
  - core:supporter-agent
  - core:host-state
```

Opening a Blueprint explains each selected node with its own properties and bounded, redacted read-only code in its exact creation scope. The node Files section resolves literal paths from its stored definition against its creation directory, checks disk presence without executing code, and opens or reveals the exact resolved path. Missing files stay visible and unavailable. Status and schedule remain in the graph header. A root accepted observation creates a persisted run identity; activating a stage carries that identity and the exact parent transition and handler into its downstream transitions. An already running target retains its first activation identity. Run grouping never infers causality from timestamps, and missing retained ancestors remain explicit. The Runs control reveals retained whole runs and replays their recorded routes with a small travelling dot; reduced-motion preferences keep the path static. Live run changes update a small existing-style toast, coalesced per Blueprint with bounded repeat suppression. Its compact path respects reduced motion and opens the full graph with that exact run selected. The workspace overview does not expose definitions. Execution output, environment and credentials remain private. Accepted decisions distinguish pattern matches from actual adapter use; configured fallback is separate, and incomplete decision history never implies that an adapter was unused. Missing origin evidence stays unknown.
