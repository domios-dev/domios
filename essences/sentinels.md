# Sentinels: self-improving action programs

A Sentinel graph is an agent's recurring action written once as a program. It runs along its own pre-planned cycle without a reasoning agent's turn, judges where judgment is needed, and hands any case it cannot handle to an explicit escalation recipient. Each case a Team member resolves can become a case the graph handles next time, so a graph improves with use. The [Supporter Agent](supporter-agent.md) supplies the judgments; the [capability map](capabilities.md) records what is in rollout and what is planned.

## Shape and cost

A graph is made of deterministic steps and decision points. A decision point offers a finite set of options, each defined in code before the graph runs. The connected decision model chooses one, and code executes exactly the chosen option. The model never constructs an option, command, target, key sequence or text, and observed output never becomes one. Without a connected adapter, author-declared match rules choose; a graph with neither is refused.

Probes have bounded runtime and output, and their cadence is requested work, not a sleep coordinating readiness. An unchanged observation never calls the model; a changed one costs one choice in the shared Supporter accounting. A transition acts once.

## What a graph may do

A graph may act on anything its owning agent may: observe, run creator-authored commands, message tabs, trigger crons, activate stages, answer provider-declared prompt options under the [prompt-answer rule](supporter-agent.md#provider-declared-prompt-answers), and stop. It never exceeds that agent's authority or its creator, Team, workspace and machine scope. Timeouts and finite transition and action budgets bound every graph; exhausting one stops the graph with a recorded reason. Each action is claimed durably before it runs and is never replayed after a restart; an uncertain outcome stays unconfirmed.

## Other, escalation and rewrite

Any decision can resolve to `other`: an unknown outcome that fits none of the offered options or falls below the confidence threshold. Reserved `other` handlers may notify or escalate to the creator or an exact same-Team agent tab. The escalation packet carries only the Sentinel and transition IDs. Adapter and budget failures never escalate.

An escalation holds that stage without another classification until the exact transition is resolved, a new run is bound, or the configured escalation timeout expires. Expiry is journaled. Each escalation cause can be delivered at most once per five minutes per Sentinel, and a durable claim is never replayed after restart. Resolution clears the observation hash and re-evaluates once.

The owning Team may finish the case and amend the graph so it becomes an offered option with its own code-defined action. Graphs are versioned: an amendment names the version it expects to replace, a stale expectation is refused, and each version keeps its author, cause and replaced definition. Any caller in the same Team and workspace may amend. The decision model never rewrites a graph.

## Ownership and lifetime

A graph belongs to its creator and Team on one machine; remote execution is unavailable. It stops on its declared completion, timeout, budget exhaustion or stop, when its creator is removed or its Team changes, and it is removed when its Team closes. An unreadable store leaves Sentinels unavailable rather than presenting an empty healthy graph.

```map
anchors:
  - capability:sentinels
  - capability:scheduling
  - core:supporter-agent
  - core:host-state
```
