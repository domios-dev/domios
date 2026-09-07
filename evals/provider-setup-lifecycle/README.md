# Provider setup lifecycle oracle

This canonical-only drill verifies the three provider-setup promises against an
exact running build SHA. Supply an opaque task value; the drill never writes a
machine-global provider hook file.

```bash
PROVIDER_SETUP_TASK_TOKEN='<opaque>' bash evals/provider-setup-lifecycle/run.sh --check
```

The drill subscribes to the dev event stream before mutations. It installs a
unique eval-local provider through `dc plugin install --from-dir`, selects it as
the configured General Agent, and proves the exact task reached its receiver in
the spawned Tab. The installer row calls the real REST route, observes its Tab
and PTY plus command execution, then uses the authenticated debug-only refusal
seam to prove the same transaction removes both provisional resources.

Each event observer receives the resolved dev URL and token as explicit opaque
arguments and owns an independent process pipe, so the persistent receiver and
subscriptions coexist without inherited production connection state. A random
run id scopes every generated name, directory, and secondary token.

The Codex row calls the existing integration-status API and accepts only a full
owned SessionStart/Stop contract or the status field that drives the Settings
consent action. It also verifies that the entire hooks file remains byte-identical
across the read and refused remote-install request. Cleanup restores the prior
General-Agent provider, closes only Tabs and the `existing=false` workspace whose
creation was proved, removes only the installed unique provider and generated
directory, then stops the receiver. Every cleanup step verifies absence; the EXIT
trap preserves the triggering status and turns cleanup residue into a nonzero result.
