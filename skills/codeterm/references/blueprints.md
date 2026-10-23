# Blueprints for recurring actions

Before you repeat a check or wait on an external, agent or cron status, apply a
Blueprint: what is watched → what happens. One verb covers built-ins and files:

```bash
domios blueprint recipe                                   # built-in and saved solutions
domios blueprint new --intent "watch CI and report failure" --json
domios blueprint keep <id> --when "Watch CI and report failure" --when "ping me when a build goes red" --when "tell me once a check fails"
domios blueprint apply github --set repo=owner/repo --set id=42 --note "Report CI failures"
domios blueprint new watch > watch.yaml                   # commented scaffold for a custom graph
domios blueprint check watch.yaml --set ...               # offline, errors at file:line:column
domios blueprint apply watch.yaml --set ... --dry-run     # then without --dry-run
```

Read `domios docs BLUEPRINT-AUTHORING` first. Choose a typed `call` only when a card's purpose and full signature fit. Check the authored graph and save its returned
sealed definition with retained pins/dependencies before apply. No-fit or degraded
recall keeps a scaffold for from-scratch authoring; open bodies with `blueprint show`.

Each code or command step runs with the user's own permissions, like an agent in a tab, without Domios credentials in its environment; foreign Blueprints are gated by folder trust (1.15.4).

Apply updates the Blueprint with the same name in your Team instead of creating a
duplicate, and reactivates a stopped one; retarget an existing watcher by applying
again with new inputs. Give every Blueprint a short `--note`. Stopped Blueprints
are deleted after 30 minutes. `domios docs BLUEPRINTS` has the file format
(inputs, `watch` or `pipeline`, `uses:`/`with:` built-in nodes, `run:` commands,
routes) and `domios docs BLUEPRINT-RECIPES` the patterns.

Creation is admission, not proof of monitoring. Run the read-only probe once and
check its exit status and exact output, including LF/CRLF. After applying, use
`blueprint show` to verify an actual source execution and the expected recognized
bucket; when a route fires, verify its retained transition and handler result.
An unclassified watch needs repair before reporting success. Continue through
events or routed results, never a sleep or polling loop. For a GitHub run use the
`github` recipe, for PR checks `github-pr-checks`; patterns need `(?m)` line anchors.

<!-- revision: 14 -->
