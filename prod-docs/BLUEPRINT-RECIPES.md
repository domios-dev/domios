# Blueprint recipes

A recipe is a native schema_version: 2 YAML Blueprint with typed `inputs`. Start with `blueprint new --intent` and reuse a fitting typed call card; otherwise apply a named recipe or author a graph. The shared cookbook is `domios docs BLUEPRINT-AUTHORING`.

```bash
domios blueprint new --intent "watch CI and report failures" --json
domios blueprint recipe
domios blueprint apply github --set repo=owner/repo --set id=42
domios blueprint new http-health > health.yaml
domios blueprint check health.yaml --set url=https://example.com/health
domios blueprint apply health.yaml --set url=https://example.com/health --dry-run
```

Use `--note` to explain the particular watch. `check` is offline; `apply --dry-run` also checks live Team scope, destinations and installed capabilities. Applying by exact name updates the existing graph instead of adding another.

| Recipe | Purpose | Required inputs |
|---|---|---|
| `github` | Run/jobs, including early failing-job reports | `repo`, integer `id` |
| `github-pr-checks` | One PR check set | `repo`, integer PR `id` |
| `github-runners` | Repository runner availability | `repo` |
| `github-org-runners` | Organization runner availability | `org` |
| `http-health` | Endpoint up/down and response code | `url` |
| `github-pr-ci-watch` | Pattern-based PR verdict watch | `name`, `pr_verdict_probe` |
| `team-watch` | Observe one member's tab | `tab_id`, `changed_pattern` |
| `wait-for-build` | Notify on a terminal build verdict | `name`, `read_only_build_probe`, `passed_pattern`, `failed_pattern` |
| `retry-until-pass` | Watch a bounded retry process | `name`, `retry_status_probe`, `passed_pattern` |
| `prompt-responder` | Request safe prompt handling | `tab_id`, `permission_pattern` |
| `cron-run-watch` | Observe a verified cron run | `cron_name_or_id`, `finished_pattern` |
| `launch-prompt-gate` | Safely handle launch prompts; escalate unknown ones | `cron_name_or_id` |
| `echo` | Echo text input through a filter | none; optional `message` |
| `all-finished` | One verdict probe for all work | `name`, `one_verdict_probe`, `all_terminal_pattern` |
| `voice-note-to-text` | Input data and conversion skeleton | `name`, absolute `path_to_voice_note` |

GitHub's `resource` input accepts `run` or `jobs`; PR checks have their own recipe because their outcome set differs. Runners accept an optional `label`. `interval` and `timeout_ms` inputs have bounded defaults. HTTP accepts `expected_status` (`2xx` or a code) and a timeout up to 10000ms. Redirects are not followed.

```bash
domios blueprint apply github --set repo=owner/repo --set id=42 --set resource=jobs
domios blueprint apply github-pr-checks --set repo=owner/repo --set id=7
domios blueprint apply github-runners --set repo=owner/repo --set label=linux-build
domios blueprint apply github-org-runners --set org=organization --set label=linux-build
domios blueprint apply http-health --set url=https://example.com/health --set expected_status=204
```

Edit each node's `routes` to name a later node, `@next` or `@stop`. Use explicit `{lit: ...}`, `{input: ...}` and `{bind: ...}` values. Commands are fixed whole values; message data is never inserted into executable text.

Watch sources, decisions and effects are separate nodes. Probe sources use `with.command`; observations use `with.kind` (`tab` or `cron_run`) and `with.id`. A decision routes a waiting bucket to `@stop` for silence and a terminal bucket to a later notify node. Deterministic patterns should match one stable verdict and handle CRLF. An `other` route may reach an escalate node with a positive escalation timeout declared on the decision.

Built-in names remain stable as inputs change, so re-apply retargets the existing named graph. Probe-based recipes retain an explicit `name` input. Creation proves admission; use `show` and retained transition/handler results to confirm an observed result.

Full format, validation diagnostics, editor schema and sharing: `domios docs BLUEPRINTS`.

## Read a web page into the next node

The Browser plugin's `browser.read_page` node opens a browser for the run, reads the page text and closes the browser. `domios blueprint nodes` lists it alongside `browser.snapshot_page`, which returns an accessibility snapshot. The code node below gets the redacted page text and returns its first line.

```yaml
- id: read
  uses: browser.read_page
  with: {url: {lit: "https://example.com"}, limit: {lit: 50}}
  outcomes: [read, navigation_failed, needs_human, browser_busy, profile_not_allowed, origin_not_allowed]
  routes: {read: first_line, navigation_failed: "@stop", needs_human: "@stop", browser_busy: "@stop", profile_not_allowed: "@stop", origin_not_allowed: "@stop"}
- id: first_line
  code: {lang: python, src: 'def main(value): return value.splitlines()[0]'}
  bind: {in: read.out.text}
```

Use `bind: {url: prior.out}` for a runtime URL produced by a step with a string output. The bound URL takes precedence over the literal. Both nodes accept an HTTP(S) URL of up to 2048 characters and optional `wait_text`, `profile` and `limit`. The default limit is 200, with a maximum of 2000 text lines or 256 snapshot entries.

Omit `profile` for a temporary browser. To use an existing named profile, set both grants from an owner-authenticated shell.

```bash
domios config set plugins.browser.blueprint_profiles '["Work"]'
domios config set plugins.browser.blueprint_origins '{"Work":["https://example.com"]}'
```

Origin grants apply to navigation and redirects. Each grant command replaces its complete setting. Set the profile list to `[]` to revoke access to all profiles. A busy profile returns `browser_busy`. Missing grants return `profile_not_allowed` or `origin_not_allowed`. The node returns `needs_human` for a login prompt, captcha or dialog, and `navigation_failed` when the page cannot be read.

Both nodes return `profile`, `final_url` and an opaque `capture_id` for each successful capture. `browser.snapshot_page` returns a JSON `snapshot` with `items` and `truncated`. Items contain `role`, `name` and `depth`, plus an absolute HTTP(S) `href` for links and a string `value` where available. Bind the JSON as `listing.out.snapshot` when the snapshot step ID is `listing`. On failure, `text` or `snapshot.error` carries the reason and `capture_id` is empty.

<!-- revision: 20 -->
