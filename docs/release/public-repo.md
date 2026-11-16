# Public repository hardening

The public source repository is `domios-dev/domios`. The owner reviews and applies this handoff before enabling public Actions or changing its policy. The worker changes no repository settings, runners, secrets, tags or remote branches.

## Apply and verify policy

From the reviewed checkout, select the personal account for this command without changing the global active account:

```bash
GH_TOKEN="$(gh auth token --user rollacode)" bash .github/admin/public-repo.sh --apply
```

The script contains the exact `gh api` requests. It disables Actions first and leaves them disabled on success or failure. Separate rulesets restrict branch creation/update to the individual owner while retaining non-bypassable PR/check, deletion and force-push rules. Trusted dev branches are also owner-only because they run code on the build farm.

Master requires a release PR and two status contexts, both pinned to the GitHub Actions app: **Release qualification** and **PR result**. Only `release-pr.yml` emits Release qualification, and only for a PR whose base is master, so a develop PR or a diagnostic dispatch on the same head SHA can never satisfy it. A base retarget arrives as `edited` and reruns the reader. The three aggregates carry distinct names: `PR result` (tests-side.yml), `CI result` (tests.yml release push) and `CI diagnostic result` (tests.yml dispatch); none of them alone admits a master merge. The reader and `scripts/ci-workflow-api.mjs` come from the PR head, so this gate guards against owner mistakes; the boundary against a hostile PR is that only the owner can merge.

Release branches carry no required status check: their checks exist only after the push, so such a rule rejects every direct release push. The script deletes a previously applied `release-required-checks` ruleset. Release heads stay owner-only through `owner-only-trusted-branches`; qualification gates the master PR and publication.

Inspect existing rulesets and environment branch policies first. The named rulesets are updated in place. The script reconciles the `release` environment's deployment branch policies to exactly two branch entries and deletes every other branch or tag policy: `release/*` (release push dry run and owner dispatches) and `master` (publication). Verify that the final list contains only those two and that the only reviewer is `rollacode`. Self-review must remain enabled so the sole owner can approve their own run. Disable environment administrator bypass in the environment settings if available.

If develop has a live rule requiring the old `CI result` context, the owner replaces it with `PR result`; a rule naming a context no PR emits blocks every develop merge.

CODEOWNERS declares review ownership; it does not independently restrict merges. The separate code-owner review ruleset lets only rollacode bypass review through a PR, because GitHub forbids self-approval. The independent master PR/status ruleset has no bypass, so even the owner must open a PR and pass its checks. Verify effective rules with an owner-authored release PR before publication. Use a narrowly scoped owner credential for automated promotion; a bot/App release token is deliberately unable to update protected branches. Remove automatic release-branch deletion from promotion because deletion is forbidden.

Non-owner writers must have no same-repository branch creation access: a same-repository PR can request a writable `GITHUB_TOKEN` in a modified workflow, regardless of the default read permission. For this source repository, remove non-owner collaborators and pending write invitations; public readers can contribute through forks. The script refuses to continue if either remains. Review identities first, then issue each removal explicitly:

```bash
gh api 'repos/domios-dev/domios/collaborators?affiliation=all' --paginate --jq '.[] | {login,permissions}'
gh api repos/domios-dev/domios/invitations --paginate
gh api --method DELETE repos/domios-dev/domios/collaborators/REVIEWED_NONOWNER_LOGIN
gh api --method DELETE repos/domios-dev/domios/invitations/REVIEWED_INVITATION_ID
gh api repos/domios-dev/domios/keys --jq '.[] | {id,title,read_only}'
gh api --method DELETE repos/domios-dev/domios/keys/REVIEWED_WRITABLE_KEY_ID
```

Audit installed GitHub Apps and token grants with repository write access; revoke any untrusted writer before re-enabling Actions. The organization alternative must likewise deny non-owner write grants. Branch restrictions alone do not constrain a malicious workflow created on an unprotected feature branch.

## CI environment

Only release jobs use an environment (`release`). Other jobs read repository-level secrets: the public client configuration, the S3/sccache keys, the store addresses and the webhook token. GitHub withholds them from fork pull requests, and privileged CI jobs reject every PR event regardless of actor. PR light checks and the qualification reader use only hosted runners and read-only tokens; `tests-side.yml` and `release-pr.yml` are the only `pull_request` workflows. The Docker archive smoke needs the build configuration, so it is an owner dispatch from release/X.Y.Z and never a PR job. The warning-summary reusable workflow selects `release` on release branches and no environment otherwise.

## Secret inventory

Before enabling public Actions, move signing keys, release tokens, Firebase/build configuration, S3/cache credentials and webhook credentials out of repository/organization-wide secret scope into their approved environment. An environment declaration does not protect a duplicate repository secret from another workflow. Inventory names without values:

```bash
gh secret list --repo domios-dev/domios
gh secret list --repo domios-dev/domios --env release
gh secret list --repo domios-dev/domios --env ci
```

Re-provision each environment secret from the owner's secret store through stdin, then remove its repository duplicate; GitHub cannot read existing secret values back:

```bash
gh secret set SECRET_NAME --repo domios-dev/domios --env release < /owner/private/secret-value
gh secret delete SECRET_NAME --repo domios-dev/domios
```

Audit organization secret repository grants and persistent runner credentials separately. Deleting a workflow secret does not remove credentials already installed on a runner. Approve only code and source inputs the owner has reviewed.

## Runner isolation is a publicization prerequisite

GitHub's [runner group REST API](https://docs.github.com/en/rest/actions/self-hosted-runner-groups) is organization-scoped. Personal repository runners have no equivalent workflow/ref allowlist. A malicious PR can modify a workflow to request self-hosted labels, so source guards alone cannot establish isolation on an unrestricted repository runner. Outside-contributor approval is an additional gate, not permission to execute their PR on the build farm.

Remove unrestricted repository runners before enabling public self-hosted Actions. Use the organization's workflow-restricted runner group for trusted builds. Owner-run removal commands (first review IDs; do not run against unidentified runners):

```bash
gh api repos/domios-dev/domios/actions/runners --paginate --jq '.runners[] | [.id,.name,.status]'
gh api --method DELETE repos/domios-dev/domios/actions/runners/REVIEWED_RUNNER_ID
gh api repos/domios-dev/domios/actions/runners --jq '.total_count' # must be 0
```

The authoritative release runner readback procedure is read-only:

```bash
gh api orgs/domios-dev/actions/runner-groups --paginate --jq '.runner_groups[] | {id,name,visibility,allows_public_repositories,restricted_to_workflows,selected_workflows}'
gh api orgs/domios-dev/actions/runner-groups/REVIEWED_GROUP_ID
gh api orgs/domios-dev/actions/runner-groups/REVIEWED_GROUP_ID/repositories
gh api orgs/domios-dev/actions/runner-groups/REVIEWED_GROUP_ID/runners
gh api repos/domios-dev/domios/actions/runners --paginate
```

Select `domios-trusted` by its returned ID. The owner must apply the reviewed
[runner-group PATCH payload](../../.github/admin/runner-group-policy.json) before requesting runners.
For each application release, retarget every acquiring workflow in that payload to the exact
`refs/heads/release/X.Y.Z` branch. Include reusable workflows as well as their callers. The prepared
1.20.0 payload uses `refs/heads/release/1.20.0`; it has not been applied by this change.

The payload must list every workflow that can select a `vars.*` runner; an unlisted one queues until
timeout once the policy is live. List the candidates and compare them with the payload:

```bash
grep -lE "vars\.[A-Z0-9_]*RUNNER" .github/workflows/*.yml
```

Workflows that run on the default branch cannot match a release ref, so they name hosted runners
literally and stay out of the payload: `dep-audit.yml` (schedule), `wiki-sync.yml` (`workflow_run`),
every `pull_request` workflow, and the master publication jobs of `release.yml`,
`release-manifest-validate.yml` and `rust-warnings-summary.yml`. The allowlist therefore needs no master
ref. Entries pin a branch, so isolation equals "who can push that branch": apply and read back the
`owner-only-trusted-branches` ruleset before the runner policy.

Owner operation, after reviewing the ID and payload:

```bash
gh api --method PATCH orgs/domios-dev/actions/runner-groups/REVIEWED_GROUP_ID --input .github/admin/runner-group-policy.json
gh api orgs/domios-dev/actions/runner-groups/REVIEWED_GROUP_ID --jq '{name,visibility,allows_public_repositories,restricted_to_workflows,selected_workflows}'
gh api orgs/domios-dev/actions/runner-groups/REVIEWED_GROUP_ID/repositories --jq '.repositories[].full_name'
gh api orgs/domios-dev/actions/runner-groups/REVIEWED_GROUP_ID/runners --jq '.runners[] | {id,name,status,labels}'
gh api repos/domios-dev/domios/actions/runners --jq '.total_count'
```

Require selected-repository access only for `domios-dev/domios`, public-repository access enabled,
`restricted_to_workflows=true`, and selected workflows equal to the reviewed payload, with no wildcard,
PR or unrelated branch refs. Require zero unrestricted repository runners. Inspect fleet membership
and record the actual readback. A prepared payload or local tests do not prove enforcement.
Never edit group policy, registrations or secrets as an agent release-preparation side effect.
Owner approval for outside contributors alone does not establish workflow isolation.

Some GitHub plans support environment reviewers only on public repositories. If policy application refuses while private, keep Actions disabled across the separately owner-approved visibility change, then apply and verify the protections. After the owner has reviewed runner isolation, secret scope and all effective settings, the final enable command is:

```bash
GH_TOKEN="$(gh auth token --user rollacode)" gh api --method PUT repos/domios-dev/domios/actions/permissions -F enabled=true
```

## Release commands

Owner release pushes start the full tests.yml qualification graph and reusable release.yml dry-run
packaging together in one push run, with per-branch concurrency and exact `github.sha` source.
Only exact release/X.Y.Z pushes are admitted. All privileged reusable jobs reject PR events.
Release PRs into master use hosted, secret-free light checks (`PR result`) and the hosted read-only
Release qualification job of `release-pr.yml`. Open or refresh the PR only after the push run is completed green; the reader
fails on missing, failed, expired or superseded same-head evidence. No workflow_run reader or
default-branch deployment is required.

Application publication has one supported path: the master merge push of the owner release PR.
It finds its merged PR and selects the green push run by exact SHA, branch, repository and owner
actors. `promote_run_id` pins receipt lookup and retained packaging to that run. Qualification,
attempt provenance, expiry, warnings, all platform and Debian receipts, manifest validation, the
protected `release` environment (branch policy `master`) and the publication switch remain required.
Every master publication job runs on hosted Ubuntu. A failed publication is repaired by rerunning the
failed jobs of that same master push run.

Dispatches never publish an application release: a `release.yml` dispatch refuses unless
`dry_run=true`, takes no `promote_run_id` and always builds the head of the dispatching release/X.Y.Z
branch; `release-publish.yml` refuses unless `rehearsal=true`. Diagnostics cannot qualify a SHA, and
a `tests.yml` dispatch admits only a full 40-hex `source_ref` already reachable from master, develop
or the dispatching release/X.Y.Z branch, so a contributor's PR head never runs on the farm.
Tag-triggered plugin/SDK publication remains disabled. Plugin dispatch takes an existing plugin tag
resolving to the reviewed branch SHA; SDK publication uses its committed package version. Original
and rerun actors must both be the owner.

```bash
gh workflow run release.yml --repo domios-dev/domios --ref release/X.Y.Z -f tag=vX.Y.Z -f dry_run=true -f mirror=false
gh workflow run release-publish.yml --repo domios-dev/domios --ref release/X.Y.Z -f run_id=REVIEWED_RUN_ID -f tag=vX.Y.Z -f rehearsal=true
gh workflow run tests.yml --repo domios-dev/domios --ref release/X.Y.Z -f source_ref=FULL_40_HEX_SHA -f scope=fast
gh workflow run docker-smoke.yml --repo domios-dev/domios --ref release/X.Y.Z
gh workflow run plugin-release.yml --repo domios-dev/domios --ref release/X.Y.Z -f tag=plugin-ID-vVERSION
gh workflow run publish-plugin-sdk.yml --repo domios-dev/domios --ref release/X.Y.Z
```

Settings APIs were checked against GitHub's [rulesets](https://docs.github.com/en/rest/repos/rules), [environments](https://docs.github.com/en/rest/deployments/environments), and [Actions permissions](https://docs.github.com/en/rest/actions/permissions) documentation. Owner application and read-back are still required; local contract tests do not prove remote policy enforcement.
