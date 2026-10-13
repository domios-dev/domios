# Permissions: who authorizes what

Domios keeps execution access, role authority, plugin grants, folder trust and model classification distinct. This contract states their confirmed boundaries; the [orchestration contract](orchestration.md) owns task and Team authority, and the [core](core.md) names the services that enforce them.

## Agent execution access

The owner chooses agent execution access through the global `agent.full_access` setting. Spawn callers cannot override that choice. Until the owner has chosen, spawn is refused before a tab is created and directs the owner to choose access in Domios.

Without full access, a model whose provider declares no unattended permission mode receives launch-scoped grants for its role's Domios commands. Launching an Orchestrator or Planner on such a model is refused before tab creation, with the model named; Team pairing checks both members first. Provider capabilities determine this behavior, not provider identity.

Command and tool permission prompts follow the tab's own permission policy. At an agent's launch or during a cron run, a prompt offering provider-declared options may be answered by one confident model choice of a declared option under the [Supporter prompt-answer rule](supporter-agent.md#provider-declared-prompt-answers); otherwise a model observation never approves it. Notification audience controls who is alerted, not execution access.

## Operator and role authority

The owner's execution-access choice is separate from authority over Teams, plans, tasks and reports. A role or task grants its defined scope through the [orchestration contract](orchestration.md); a provider execution mode does not confer another role. An agent cannot claim the owner's identity merely by supplying a command or request field. Human confirmation and agent role checks remain distinct at the operation that requires them.

## Folder trust and provenance

Trust in a directory authorizes the provider-declared folder-trust response, not blanket approval of future commands. It is established by the existing deterministic policy: host-recorded provenance, shared trust, or a live same-provider tab with a verified session in the exact canonical directory. At an agent's launch or during a cron run, the provider-declared folder-trust option may also be chosen under the [Supporter prompt-answer rule](supporter-agent.md#provider-declared-prompt-answers). Otherwise, uncovered folder trust requires owner attention.

Hidden memory workers trust their daemon-owned folder. Crons trust the folder chosen by the owner when the schedule was created. Provenance covers that exact directory; an arbitrary supplied path or model verdict cannot create it. This provenance handling is merged for 1.12.4, in rollout.

## Plugin grants and integration consent

Plugins declare capabilities and permissions. The host registers only capabilities it supports, and declared permissions bound access to local files, subprocesses, network hosts and secrets. A plugin update cannot supply host behavior absent from the installed app. Plugin confinement and official-channel provenance remain under the [plugin-host contract](core.md#core:plugin-host).

Plugin actions that require a missing sensitive grant show a host-owned consent
prompt naming the plugin and access, with Allow and Not now. Allow persists the
same decision as CLI configuration, applies it and retries the refused action
once. UI installation and updates ask once for each new sensitive permission;
Not now is retained. Plugin settings list declared sensitive permissions with
allowed states and reversible toggles beside network access. Extra network hosts
use the same explicit consent boundary and remain exact-host grants.

A declared local recording trigger requires an explicit user grant. System trust,
development access, and a manifest request do not supply it. The host owns selected
devices and recipient selection; the plugin can inspect its own sanitized status
and arm or disable its trigger without receiving raw input, audio, or authority to
send to arbitrary tabs. START freezes an authenticated ready recipient and one
capture generation. A closed explicit choice remains stale; General resolves only
at START when no explicit choice exists. Cancellation, device loss, revocation and
unload invalidate capture authority. Successful typed transcription enters the
ordinary owner voice pipeline once, with exact recipient identity checked again
before admission and handoff. Unsupported capture surfaces refuse the operation.

Plugin setup guidance is data. A helper asks the owner what to change and uses the existing permission-gated configuration path only after confirmation. Global provider integrations may be inspected without mutation; installing or repairing them requires explicit user consent. Domios changes only entries it owns and provides a reversible restore path. Only the machine-owner instance reclaims unused launch profiles, preserving provider trust first. Live profiles and unknown foreign instances are kept; a registered foreign instance is reclaimable only while its exclusive lifetime lock is held. One-time legacy cleanup matches only generated filenames, keeps live tabs and reports counts to the owner.

## Supporter classification

The [Supporter Agent](supporter-agent.md) names a state or ranks a candidate. Its output cannot grant execution access, folder trust, plugin permissions, operator authority or session ownership. Deterministic evidence and the applicable permission policy authorize the resulting operation. Connecting an adapter does not broaden any grant.

```map
anchors:
  - core:agents-and-providers
  - core:plugin-host
  - core:host-state
  - core:supporter-agent
  - capability:agent-providers
  - capability:provider-integrations
  - capability:plugins-and-channels
  - capability:teams-and-roles
```
