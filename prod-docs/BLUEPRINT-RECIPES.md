# Blueprint recipes

A recipe is a YAML Blueprint with typed `inputs`. Apply a built-in directly for common work; print its file when you need a custom graph.

```bash
codeterm blueprint recipe
codeterm blueprint apply github --set repo=owner/repo --set id=42
codeterm blueprint new http-health > health.yaml
codeterm blueprint check health.yaml --set url=https://example.com/health
codeterm blueprint apply health.yaml --set url=https://example.com/health --dry-run
```

Use `--note` to explain the particular watch. `check` is offline; `apply --dry-run` also checks live Team scope, destinations and installed capabilities. Applying by exact name updates the existing graph instead of adding another.

| Recipe | Purpose | Required inputs |
|---|---|---|
| `github` | Run/jobs, including early failing-job reports | `repo`, integer `id` |
| `github-pr-checks` | One PR check set | `repo`, integer PR `id` |
| `github-runners` | Repository runner availability | `repo` |
| `github-org-runners` | Organization runner availability | `org` |
| `http-health` | Endpoint up/down and response code | `url` |
| `github-pr-ci-watch` | Pattern-based PR verdict watch | `pr_verdict_probe` |
| `team-watch` | Observe one member's tab | `tab_id`, `changed_pattern` |
| `wait-for-build` | Notify on a terminal build verdict | `read_only_build_probe`, `passed_pattern`, `failed_pattern` |
| `retry-until-pass` | Watch a bounded retry process | `retry_status_probe`, `passed_pattern` |
| `prompt-responder` | Request safe prompt handling | `tab_id`, `permission_pattern` |
| `cron-run-watch` | Observe a verified cron run | `cron_name_or_id`, `finished_pattern` |
| `launch-prompt-gate` | Safely handle launch prompts; escalate unknown ones | `cron_name_or_id` |
| `echo` | Command followed by a filter | none; optional `message` |
| `all-finished` | One verdict probe for all work | `one_verdict_probe`, `all_terminal_pattern` |
| `voice-note-to-text` | Input data and conversion skeleton | absolute `path_to_voice_note` |

GitHub's `resource` input accepts `run` or `jobs`; PR checks have their own recipe because their outcome set differs. Runners accept an optional `label`. `interval` and `timeout_ms` inputs have bounded defaults. HTTP accepts `expected_status` (`2xx` or a code) and a timeout up to 10000ms. Redirects are not followed.

```bash
codeterm blueprint apply github --set repo=owner/repo --set id=42 --set resource=jobs
codeterm blueprint apply github-pr-checks --set repo=owner/repo --set id=7
codeterm blueprint apply github-runners --set repo=owner/repo --set label=linux-build
codeterm blueprint apply github-org-runners --set org=organization --set label=linux-build
codeterm blueprint apply http-health --set url=https://example.com/health --set expected_status=204
```

Edit the file's `on` routes when composing branches. Keep inputs for reusable parameters, and use a custom `run` step only when a built-in node does not fit. A command returns text; HTTP and GitHub expose fixed outcomes. `on: {done: stop, failed: diagnose}` routes to a later `diagnose` stage.

For watch patterns, probes should print one stable verdict, not timestamps or long logs. Keep waiting states silent with `routes: {waiting: none}`. Define a meaningful question and patterns anchored per line (`(?m)`); errors must never resemble success. Pattern watches never fall back to the model. Creation proves admission; use `show` and retained transition/handler results to confirm a real observed result.

Watch same-Team tabs with `observe: tab:<id>`, or a cron's exact verified run with `observe: cron-run:<name-or-id>`. A shell requires an explicit terminal pattern or idle bound. An agent status watch does not type into the observed tab. To react to repeated changed output in one bucket, declare `rearm_on_change: [failed]`.

Unknown cases can use `handlers.other` to escalate to the creator or a same-Team agent. Repair the YAML and apply it, then resolve the held transition. The model chooses a finite bucket; fixed code-defined handlers own actions and targets.

Full format, validation diagnostics, editor schema and sharing: `codeterm docs BLUEPRINTS`.

<!-- revision: 13 -->
