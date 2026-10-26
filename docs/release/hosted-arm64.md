# Hosted arm64 compilation

The release graph keeps its six platform callers and qualification, publication and
promotion validators. Linux arm64 and Windows arm64 each call
`.github/actions/arm64-compile` inside the signing job on the same hosted arm64 runner. The other
four platforms keep their current paths.

| Platform | Compile, unsigned | Finalize, sign and verify |
| --- | --- | --- |
| Linux arm64 | `ubuntu-24.04-arm` | `RELEASE_LINUX_ARM64_LABEL` (default `codeterm-builder`), native Linux arm64 verification container |
| Windows arm64 | `windows-11-arm` | `RELEASE_WINDOWS_ARM64_LABEL` (default `windows-build`) |

Hosted jobs check out the exact source SHA, fetch the run's digest-pinned frontend,
build CLI/server/client once, freeze the full stage and bundle with updater artifacts
disabled. No signing, publication, S3 or Tailnet secrets are passed. Both jobs use
two Cargo workers, no incremental compilation and release debug information disabled.
The runner is discarded after the job; there is no persistent compiler cache.

The handoff tar preserves executable permissions and contains the full stage, bundles,
original server Cargo receipt and closure log. Its SHA-256 travels as the producing
job's output independently of the artifact. Finalization checks that digest before
extraction, then checks the complete file inventory, target, source, tag, run and
attempt. It refuses links, extra/changed files and hosted signatures. The existing
full-package comparison and headless-stage validators produce and read back the
headless archive locally. Bundle smoke checks architecture, embedded source, plugins,
resources and version; Windows retains its Authenticode status evidence. Signing adds
detached updater signatures and a second inventory check proves original bytes did
not change. Linux also compares its signing copies to the original hosted digests.

`unsigned-<target>-<attempt>` and `finalized-handoff-<target>-<attempt>` run artifacts
retain unsigned bytes and finalization receipts for seven days. Existing private
dry-run capture, six-platform aggregation and publication continue to own the signed
assets. Promoting a green dry run consumes those retained bytes without rebuilding or
signing again. A mismatched handoff is a failure, never a request to rebuild locally.

The frontend producer also uploads `frontend-dist-<attempt>` as a run artifact.
Hosted compilers download it through GitHub and verify the producer's SHA-256;
they never need private-store connectivity to receive the run's shared frontend.

## Owner setup

Enable Actions hosted-runner usage and adequate included minutes/spending budget for
this private repository. The selected standard arm64 runners have two CPUs and 8 GB
RAM; billable runtime and artifact storage depend on the repository's plan.
See [GitHub's runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).

The hosted compile call explicitly forwards only these existing repository secrets
as public client configuration: DOMIOS_FIREBASE_API_KEY, DOMIOS_FIREBASE_PROJECT_ID,
DOMIOS_FIREBASE_APP_ID, DOMIOS_ACCOUNT_REDEEM_URL, DOMIOS_BILLING_FUNCTIONS_URL and
DOMIOS_AMPLITUDE_API_KEY_PROD. All six are required. No repository variables are
needed. The allowlist contract rejects additional credentials; signing, S3/cache
and publication tokens remain exclusive to private finalize jobs.
Existing signing and publication secrets stay on finalize jobs. The Linux finalizer
needs its existing `codeterm-build-linux-arm64` Docker image; no image rebuild or
runner-label change is required.

No new cache secret is required for the chosen cold-cache baseline. Joining the
Tailnet would require a tagged reusable ephemeral `TAILSCALE_AUTHKEY`, restricted
S3 access credentials and an ACL limited to the cache endpoint. Tailscale's action
creates and removes an ephemeral node for each run; that adds setup, credential
rotation and private-network access. Its current recommended alternative is workload
identity federation. See [Tailscale's action documentation](https://tailscale.com/docs/integrations/github/github-action).
Cold compilation has the lowest integration and credential cost; its billing cost
must be measured before deciding whether remote cache savings justify that setup.

## One measuring dry run

After review/integration, pushing the reviewed workflow ref, and exact-SHA qualification,
record that ref's resolved SHA and dispatch once with owner authorization:

```bash
gh workflow run release.yml --repo rollacode/codeterm --ref <reviewed-ref> \
  -f tag=v<package-version> -f dry_run=true -f mirror=false
```

Record the returned run ID, workflow/source SHA, both hosted job durations and billed
minutes, finalizer durations, archive digests, artifact sizes, native smoke outcomes,
and the consolidated six-platform dry-run receipt. The 120-minute job deadlines stay
bounded; a capacity, disk or toolchain failure is evidence to review before another
run. No measuring workflow was dispatched during implementation.
