#!/usr/bin/env bash
set -euo pipefail

[[ "${1:-}" == --apply ]] || { echo 'Owner only: review docs/release/public-repo.md, then pass --apply' >&2; exit 2; }
repo=rollacode/codeterm
api_version=2026-03-10
api() { gh api -H "X-GitHub-Api-Version: $api_version" "$@"; }
[[ "$(api user --jq .login)" == rollacode ]] || { echo 'Authenticate as rollacode' >&2; exit 2; }
owner_id=$(api users/rollacode --jq .id)
api --method PUT "repos/$repo/actions/permissions" -F enabled=false
writers=$(api "repos/$repo/collaborators?affiliation=all" --paginate --jq '.[] | select(.login != "rollacode" and (.permissions.push or .permissions.maintain or .permissions.admin)) | .login')
invitations=$(api "repos/$repo/invitations" --paginate --jq '.[].id')
[[ -z "$writers" && -z "$invitations" ]] || { echo 'Remove non-owner writers and pending invitations before applying; Actions remains disabled' >&2; exit 2; }
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

upsert_ruleset() {
  local name="$1" body="$2" id
  id=$(api "repos/$repo/rulesets" --paginate --jq ".[] | select(.name == \"$name\") | .id")
  if [[ -n "$id" ]]; then
    [[ "$id" != *$'\n'* ]] || { echo 'Duplicate ruleset names; review manually' >&2; exit 2; }
    api --method PUT "repos/$repo/rulesets/$id" --input "$body"
  else
    api --method POST "repos/$repo/rulesets" --input "$body"
  fi
}

jq -n --argjson owner "$owner_id" '{name:"owner-only-trusted-branches",target:"branch",enforcement:"active",
  bypass_actors:[{actor_id:$owner,actor_type:"User",bypass_mode:"always"}],
  conditions:{ref_name:{include:["refs/heads/master","refs/heads/release/**","refs/heads/develop","refs/heads/rename/domios"],exclude:[]}},
  rules:[{type:"creation"},{type:"update"}]}' > "$scratch/owner.json"
upsert_ruleset owner-only-trusted-branches "$scratch/owner.json"

jq -n '{name:"trusted-branch-integrity",target:"branch",enforcement:"active",bypass_actors:[],
  conditions:{ref_name:{include:["refs/heads/master","refs/heads/release/**","refs/heads/develop","refs/heads/rename/domios"],exclude:[]}},
  rules:[{type:"deletion"},{type:"non_fast_forward"}]}' > "$scratch/integrity.json"
upsert_ruleset trusted-branch-integrity "$scratch/integrity.json"

jq -n '{name:"master-release-pr",target:"branch",enforcement:"active",bypass_actors:[],
  conditions:{ref_name:{include:["refs/heads/master"],exclude:[]}},
  rules:[{type:"pull_request",parameters:{required_approving_review_count:0,dismiss_stale_reviews_on_push:true,
    require_code_owner_review:false,require_last_push_approval:false,required_review_thread_resolution:true,
    allowed_merge_methods:["merge"]}},
    {type:"required_status_checks",parameters:{strict_required_status_checks_policy:true,
      required_status_checks:[{context:"CI result"}]}}]}' > "$scratch/master.json"
upsert_ruleset master-release-pr "$scratch/master.json"

jq -n --argjson owner "$owner_id" '{name:"owner-code-review",target:"branch",enforcement:"active",
  bypass_actors:[{actor_id:$owner,actor_type:"User",bypass_mode:"pull_request"}],
  conditions:{ref_name:{include:["refs/heads/master"],exclude:[]}},
  rules:[{type:"pull_request",parameters:{required_approving_review_count:0,dismiss_stale_reviews_on_push:true,
    require_code_owner_review:true,require_last_push_approval:false,required_review_thread_resolution:true,
    allowed_merge_methods:["merge"]}}]}' > "$scratch/review.json"
upsert_ruleset owner-code-review "$scratch/review.json"

jq -n '{name:"release-required-checks",target:"branch",enforcement:"active",bypass_actors:[],
  conditions:{ref_name:{include:["refs/heads/release/**"],exclude:[]}},
  rules:[{type:"required_status_checks",parameters:{strict_required_status_checks_policy:true,
    required_status_checks:[{context:"CI result"}]}}]}' > "$scratch/release.json"
upsert_ruleset release-required-checks "$scratch/release.json"

jq -n --argjson owner "$owner_id" '{wait_timer:0,prevent_self_review:false,
  reviewers:[{type:"User",id:$owner}],deployment_branch_policy:{protected_branches:false,custom_branch_policies:true}}' > "$scratch/environment.json"
api --method PUT "repos/$repo/environments/release" --input "$scratch/environment.json"
api --method POST "repos/$repo/environments/release/deployment-branch-policies" -f name='release/*' -f type=branch

api --method PUT "repos/$repo/environments/ci" --input - <<'JSON'
{"wait_timer":0,"reviewers":[],"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true}}
JSON
for branch in rename/domios develop master; do
  api --method POST "repos/$repo/environments/ci/deployment-branch-policies" -f "name=$branch" -f type=branch
done

api --method PUT "repos/$repo/actions/permissions/fork-pr-contributor-approval" -f approval_policy=all_external_contributors
api --method PUT "repos/$repo/actions/permissions/workflow" -f default_workflow_permissions=read -F can_approve_pull_request_reviews=false
# This endpoint governs private forks during the transition, not public-fork runner isolation.
api --method PUT "repos/$repo/actions/permissions/fork-pr-workflows-private-repos" \
  -F run_workflows_from_fork_pull_requests=false -F send_write_tokens_to_workflows=false \
  -F send_secrets_and_variables=false -F require_approval_for_fork_pr_workflows=true

api "repos/$repo/rulesets" --paginate
api "repos/$repo/environments/release"
api "repos/$repo/environments/release/deployment-branch-policies"
api "repos/$repo/environments/ci"
api "repos/$repo/environments/ci/deployment-branch-policies"
api "repos/$repo/actions/permissions/fork-pr-contributor-approval"
api "repos/$repo/actions/permissions/workflow"
api "repos/$repo/actions/runners" --paginate
