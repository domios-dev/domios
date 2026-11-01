# Public repository hardening

The public source repository is `domios-dev/domios`. The owner reviews and applies this handoff before enabling public Actions or changing its policy. The worker changes no repository settings, runners, secrets, tags or remote branches.

## Apply and verify policy

From the reviewed checkout, select the personal account for this command without changing the global active account:

```bash
GH_TOKEN="$(gh auth token --user rollacode)" bash .github/admin/public-repo.sh --apply
```

The script contains the exact `gh api` requests. It disables Actions first and leaves them disabled on success or failure. Separate rulesets restrict branch creation/update to the individual owner while retaining non-bypassable PR/check, deletion and force-push rules. Trusted dev branches are also owner-only because they run code on the build farm. Master requires a release PR and the always-present source and secret-scan checks. Release branches require secret-scan, which runs on release pushes and trusted dev pushes before branch creation. Other path-filtered checks are not required because absent checks would block merges indefinitely. Exact-SHA qualification remains a release preparation requirement.

Inspect existing rulesets and environment branch policies first. The named rulesets are updated in place; the deployment branch policy POST is for initial setup. On a second application, reuse its existing policy ID rather than adding duplicates. Delete any broader existing environment branch/tag policies with `gh api --method DELETE repos/domios-dev/domios/environments/release/deployment-branch-policies/POLICY_ID`. Verify that the final list contains only the branch policy `release/*` and that the only reviewer is `rollacode`. Self-review must remain enabled so the sole owner can approve their dispatch. Disable environment administrator bypass in the environment settings if available.

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

Only release jobs use an environment (`release`). Other jobs read repository-level secrets: the public client configuration, the S3/sccache keys, the store addresses and the webhook token. GitHub withholds them from fork pull requests, and CI jobs reject every PR event regardless of actor. The warning-summary reusable workflow selects `release` on release branches and no environment otherwise.

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

The current organization is `domios-dev`. Apply runner group changes only after owner approval, with reviewed group/runner IDs. No wildcards or PR refs are admitted. Enumerate every reviewed release branch explicitly; add a branch before its first release push. Include the reusable workflows themselves, since their jobs own the runner requests.

```bash
org=domios-dev
repo="$org/domios"
group_id=REVIEWED_GROUP_ID
repo_id=$(gh api "repos/$repo" --jq .id)
gh api "orgs/$org/actions/runner-groups" --paginate
node --input-type=module - "$repo" release/REVIEWED_VERSION > runner-group.json <<'NODE'
import fs from 'node:fs';
const [repo, branch] = process.argv.slice(2);
if (!/^release\/[^/]+$/.test(branch)) throw new Error('one exact release branch required');
const workflows = fs.readdirSync('.github/workflows').filter(name => name.endsWith('.yml') &&
  fs.readFileSync(`.github/workflows/${name}`, 'utf8').includes('self-hosted'));
console.log(JSON.stringify({name:'trusted-builds', visibility:'selected', allows_public_repositories:true,
  restricted_to_workflows:true, selected_workflows:workflows.flatMap(name =>
    ['master','develop','rename/domios',branch].map(ref => `${repo}/.github/workflows/${name}@refs/heads/${ref}`))}));
NODE
gh api --method PATCH "orgs/$org/actions/runner-groups/$group_id" --input runner-group.json
gh api --method PUT "orgs/$org/actions/runner-groups/$group_id/repositories" -F "selected_repository_ids[]=$repo_id"
gh api --method PUT "orgs/$org/actions/runner-groups/$group_id/runners/REVIEWED_ORG_RUNNER_ID"
gh api "orgs/$org/actions/runner-groups/$group_id"
gh api "orgs/$org/actions/runner-groups/$group_id/repositories"
gh api "orgs/$org/actions/runner-groups/$group_id/runners"
```

Verify all fleet runners belong exclusively to the restricted group; remove repository-level registrations and broader group grants. The API output must show `restricted_to_workflows: true`, exactly the trusted refs, and only the intended repository. Until that is proven, public self-hosted Actions stays disabled.

Some GitHub plans support environment reviewers only on public repositories. If policy application refuses while private, keep Actions disabled across the separately owner-approved visibility change, then apply and verify the protections. After the owner has reviewed runner isolation, secret scope and all effective settings, the final enable command is:

```bash
GH_TOKEN="$(gh auth token --user rollacode)" gh api --method PUT repos/domios-dev/domios/actions/permissions -F enabled=true
```

## Release commands

Run dispatches from a reviewed `release/X.Y.Z` branch. Tag-triggered plugin/SDK publication and release-publish.yml automatic publication are disabled. Owner release pushes start tests.yml qualification; release.yml starts packaging only after that workflow succeeds for the same exact current release head. Its workflow_run admission rejects foreign repositories, actors, source events and branches before acquiring a release runner or secrets. Superseded heads skip packaging successfully. The exact-SHA receipt check remains mandatory in prepare, including owner dispatches. Deploy the trigger and reusable guards to the default branch to activate workflow_run; GitHub reads that event's workflow definition there. Plugin dispatch takes an existing plugin tag; the tag must resolve to the reviewed branch SHA. SDK publication uses the committed package version. All privileged release jobs require both the original and rerun actor to be the owner and the `release` environment, including reusable platform jobs and dry runs.

```bash
gh workflow run release.yml --repo domios-dev/domios --ref release/X.Y.Z -f tag=vX.Y.Z -f dry_run=true -f mirror=false
gh workflow run release-publish.yml --repo domios-dev/domios --ref release/X.Y.Z -f run_id=REVIEWED_RUN_ID -f tag=vX.Y.Z -f rehearsal=true
gh workflow run plugin-release.yml --repo domios-dev/domios --ref release/X.Y.Z -f tag=plugin-ID-vVERSION
gh workflow run publish-plugin-sdk.yml --repo domios-dev/domios --ref release/X.Y.Z
```

Settings APIs were checked against GitHub's [rulesets](https://docs.github.com/en/rest/repos/rules), [environments](https://docs.github.com/en/rest/deployments/environments), and [Actions permissions](https://docs.github.com/en/rest/actions/permissions) documentation. Owner application and read-back are still required; local contract tests do not prove remote policy enforcement.
