# Sentinels: recurring actions as self-improving graphs

A Sentinel graph is how an agent describes an action it would otherwise repeat by hand. The graph runs along its own pre-planned cycle without a reasoning agent's turn: cheap where the case is routine, still able to judge, and handing the unusual case to an agent. Every case an agent resolves is meant to become a case the graph handles next time, so a graph improves with use. The [Supporter Agent](supporter-agent.md) supplies the judgments; the [capability map](capabilities.md) records what is in rollout and what is planned.

## The shape of a graph

A graph is made of deterministic steps and decision points. Deterministic steps observe and act without a model: a bounded probe, a fixed creator-authored command, a message to a named tab, a named cron, activation of another stage, or stop. A decision point offers a finite set of options, each defined in code by the graph's author before the graph runs. The connected decision model chooses one option, and code executes exactly the chosen option.

The model never constructs an option, target, command, key sequence or message, and observed output never becomes one. Without a connected adapter, author-declared deterministic match rules make the choice; a graph with neither is refused. A graph named as one scenario shows its stages, their state and their results as one unit.

## Cost

A graph is cheaper than an agent polling. Probes have bounded runtime and output. Their cadence is requested work, not a sleep coordinating internal readiness. An unchanged observation never calls the model, and a changed one costs one classification counted in the shared Supporter accounting. A transition acts once. Finite transition and action budgets bound every graph, and exhausting them stops it with a recorded reason.

## Authority

Choosing is not authorizing. An option may do only what its author was entitled to authorize, and the model selects among those options rather than granting anything. An option that answers a provider prompt uses a response the provider declares, delivered through the existing prompt-response surface, never invented keys. Folder trust follows the [folder-trust contract](permissions.md#folder-trust-and-provenance), and command or tool permission follows the [agent execution-access contract](permissions.md#agent-execution-access). Whether a graph author may pre-authorize a permission response for unattended agents remains an open owner decision; until it is made, the permissions contract decides.

Each accepted transition and each action attempt has a durable identity. A claim is recorded before its action and is never replayed after a restart; an uncertain outcome stays unconfirmed. A graph's results confirm the operation it performed, not the recipient's work.

## Other, escalation and improvement

Every decision point can resolve to `other`: the observation fits none of the offered options, or the choice falls below its confidence threshold. In rollout today, `other`, a failed classification and an exhausted explicit cap are journaled and take no route.

The intended contract, planned and not yet built, makes `other` an escalation. The graph holds that step and attaches an agent to finish it, rather than guessing or dropping the case. Once the agent resolves the case, the graph is amended so the same case becomes an offered option with its own code-defined action, and the next occurrence runs without an agent. The escalated agent authors the amendment, not the decision model, and the amendment stays within the authority of the graph's owner. Each amendment keeps its lineage: the case that caused it, its author and the definition it replaced. Whether an amendment takes effect directly or after review by the graph's creator or the owner remains open.

The driving case is an unattended agent launched by a cron and stopped at a folder-trust or permission prompt. The graph offers the provider-declared responses, the model picks one, and code delivers it under the authority rules above. An unrecognized prompt escalates to an agent, and its resolution becomes a new offered branch.

## Ownership and lifetime

A graph belongs to its creator and Team, in the creator's workspace, on one machine. Routes and stage activations stay inside that scope, and remote execution is unavailable. A graph stops on its declared completion, timeout, budget exhaustion or stop. It stops when its creator is removed or its Team changes, and it is removed when its Team closes. An unreadable store leaves Sentinels unavailable rather than presenting an empty healthy graph. Sentinels hold no task authority or delivery authority of their own.

```map
anchors:
  - capability:sentinels
  - capability:scheduling
  - core:supporter-agent
  - core:host-state
```
