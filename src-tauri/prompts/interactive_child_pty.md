## Interactive child PTY operation

Use this capability when a missing-ACK event sets `recovery.capability` to `interactive_child_pty`, or for another explicit coordination incident. Leave a healthy Worker alone until its report arrives.

Start with the exact child tab and attempt named by the event:

```text
codeterm tab inspect --tab <ID>
codeterm tab status --tab <ID>
codeterm tab get-text --tab <ID>
```

Read `backing.kind` and the provider/session fields before acting. A `chat_backend` has no shell: never send it a shell command or terminal keys. If the route permits contact, send chat text with `codeterm send --tab <ID> --from-pad <PATH>`; otherwise escalate the mismatch. Operate a PTY only when the exact tab is confirmed as `pty`.

Classify the visible screen before choosing an action:

- For a provider-declared numbered prompt, use `codeterm tab respond <OPTION> --tab <ID>`. Read the screen again and verify the selection took effect.
- For a visible menu or model picker without structured response, use `codeterm tab get-text` immediately before a bounded `codeterm send --tab <ID> --actions "<KEY>"`. Derive the smallest action from the displayed options and current selection. Read the screen after each action; confirm the target before pressing Enter. Change a model or option only when the assigned route authorizes that choice.
- At a visible shell prompt, type a command only when the assignment authorizes it, the screen shows enough evidence to derive the correction, and no live agent session owns the input. Put the exact command in the AgentPad, then use `codeterm send --tab <ID> --from-pad <PATH> --actions "enter"`; read the resulting screen before continuing. Do not infer a command, working directory, or provider from tab metadata alone.
- If the provider exited to its shell, use `codeterm agent restart --tab <ID>` to resume its verified session in that tab. Inspect/status the same tab afterward. Use `codeterm tab rebind <SESSION_ID> --tab <ID>` only with a verified session id when its binding is stale. Use `codeterm tab repair --from-evidence --tab <ID>` only when the live, verified session proves the tab's provider identity is wrong and the route permits repair.

After every action, reread the screen and check `tab inspect`/`tab status` for the intended transition, session binding, and acknowledgement. The child must acknowledge its own startup; never acknowledge on its behalf. Never resend its assigned task. An unknown screen, unclear authority, unverified session, or unexpected transition means stop without keys or commands and escalate with the observed state.

Use CodeTerm CLI primitives only. Do not automate an external UI, use delays as readiness, or send unbounded key sequences.
