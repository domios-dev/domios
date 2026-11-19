# CI and release admission

**PR result** is the aggregate to require: tests-side.yml owns all PR light checks
into develop and master, including Dependabot and forks. Master release PRs additionally
require **Release qualification**, which only release-pr.yml emits and only for a master base.
tests.yml owns owner release pushes (**CI result**) and diagnostics (**CI diagnostic result**);
neither name is a merge gate. Repository rules remain an owner operation; changing a
workflow does not update live required-check settings.

| Event | Checks |
| --- | --- |
| Owner push to exact release/next | Full CI and reusable dry-run packaging in the same run |
| Owner release/next PR into master | Hosted PR checks and read-only same-SHA qualification |
| Owner `release/plugin-<id>` PR into master | Exact single-plugin scope and whitespace, plugin release metadata, bundles, types and tests on hosted Ubuntu. No release push or Rust jobs |
| Any PR into develop | Hosted repository guards, fmt-check, TS static checks/unit/Vitest, plugin bundle checks/tests, one workspace cargo check --all-targets, then PR result |
| Other PR into master | Existing repository refusal: master accepts only the owner's release/next or a valid single-plugin PR |
| Owner CI dispatch | tests.yml diagnostic scope from release/next, for a full 40-hex source_ref reachable from master, develop or that branch |

Every PR job uses literal ubuntu-24.04, independent of runner
variables. It requests a read-only token, no repository/environment secrets,
no release environment, no reusable release graph, no qualification and no Rust
test execution. Rust checking includes the CLI workspace member and all target
kinds on Linux; it uses the GitHub Actions sccache backend and Cargo dependency
cache without private-store credentials or endpoints. Fork/Dependabot PRs need
no store access. All normal PRs run every light check. For an owner direct plugin PR the aggregate
requires the normal jobs to be skipped, and master separately requires a green
Release qualification job with the plugin checks.

After merge, verify the first Dependabot PR into develop by its commit SHA:
expect Repository guards, fmt-check, ts static checks, ts unit, ts vitest,
plugin bundles + tests, Rust workspace check and PR result, all on GitHub-hosted
Ubuntu. Inspect the jobs API's runner names and conclusions; local actionlint
and trigger review do not prove a live hosted run.

The source guards finish successfully before either test branch can acquire compilers.
Shared guards cover sizes, provider boundary, crate layering, Tauri reach,
changed-file comments, API/prompt/eval documentation, secret scanning, issue
metadata and owner-only master PR admission. Each runner checks
its tools through ci-runner-paths before dependency work or compilation.
Hosted jobs use pinned Node/Python/pnpm setup before preflight and script use.
Self-hosted jobs use the pinned github-script action's bundled Node runtime,
then discover already-installed tools through ci-runner-paths. Installer actions
run only on GitHub-hosted runners; self-hosted images provide their role tools. Failure names the missing tools and install procedure.

CI jobs declare their exact executable/module list in preflight instead of sharing
a broad Rust or guard role. The inventory contract expands catalogue and pnpm
entrypoints and checks all four CI workflow files, including provisioning order.
Only nextest consumers install it on hosted runners; self-hosted consumers
verify the image's installed nextest. Both guard jobs install
scripts/requirements-ci.txt into a job-local virtual environment and preflight
imports every required third-party test module; the Python AST inventory rejects
new imports until their pinned dependency and module probe are declared.

All checkouts select an explicit source at depth one (depth two only for
parent-tree comparisons). Jobs that need ancestry fetch only the exact source
and the named baseline/publication refs through scripts/ci-git-history.mjs;
product tags are limited to v* and release-*. The five-minute warning receipt
resolves the develop fork point through the compare API and fetches that single
commit, so it never downloads branch history. Both guard graphs install pinned
cargo-nextest before preflight; cache fixtures set their own cache policy instead
of inheriting the runner's settings.

Normal CI uses triage mode: independent test jobs finish after a failure,
nextest and platform matrices keep fail-fast off, and packaging runs beside the test jobs, while no failing qualification
can admit publication. A newer commit can still supersede an older run.
CI result reads named failing/timed-out child jobs from the Actions API; timeouts remain failures rather than cancelling siblings. Hosted TS checks have 15-minute bounds; generated types and the private guard role have 20 minutes.

The release push runs full qualification and dry-run packaging together from `github.sha`.
Only owner pushes to `release/next` enter that graph. The exact source package.json sets the app version. Open or refresh the release
PR after this push run is green. Its hosted read-only reader validates the current PR head and
branch tip, selects the completed green tests.yml push run for the exact SHA, branch, repository
and owner actors, then validates unexpired receipts from that run. Missing, failed, foreign,
superseded or expired evidence fails; absence never passes. No PR event can call privileged jobs. Master promotion jobs are pinned to hosted Ubuntu,
so the release-only runner-group allowlist needs no master workflow refs.

An admin merges the green release PR with a merge commit, preserving its branch. The master push
requires the merged PR and qualified head tree, selects its exact owner push run and sets
promote_run_id. Qualification and packaging must belong to that same run. The master merge commit
is never built. Existing attempt provenance, expiry, warning, platform, Debian and manifest guards,
release environment and publication switch remain required.

Qualification pushes upload verified signed assets to hidden source and mirror drafts as each
platform finishes. Each run has its own draft, recording source SHA/run ID and per-platform
completion receipts. A failed run leaves its hidden partial draft; a rerun clears only that
platform's old assets. A new run supersedes older hidden drafts of its version. Dispatch dry runs
retain private evidence only and create no drafts or public artifacts.

Promotion reads the admitted run's draft metadata, then publishes that draft and the recorded
qualified manifests without downloading or re-hashing payloads. Missing or incomplete own-run
drafts use parallel per-platform retained downloads and upload concurrency six; foreign drafts
refuse. Public verification reads manifests, authenticates their signed version comments and
compares signatures, URLs and sha256 metadata with qualification, then HEAD-checks every asset's
status and size, including Debian and the mirror. Issue closure requires that evidence and the
same qualification identity. Payload signature verification remains in qualification.

The release Orchestrator pins the reviewed runner-group payload once to `release/next` under
standing owner permission. Read it back for each release.
See [the runner policy procedure](public-repo.md#runner-isolation-is-a-publicization-prerequisite).
Require restricted_to_workflows=true and exact reviewed refs, selected repository access and no
unrestricted repository runners. Preparing the payload changes no live policy. Hosted unsigned
compile jobs receive their existing public build configuration; signing credentials remain in
protected signing jobs. Branch synchronization uses an admin-owned merge PR.

Read status by commit, not by the latest run:

    source_repository=$(node scripts/source-repository.mjs)
    gh api repos/$source_repository/commits/<sha>/check-runs --jq '.check_runs[] | {name, status, conclusion, details_url}'
    gh run list --repo "$source_repository" --workflow tests.yml --commit <sha>
    node scripts/ci-qualification.mjs check <sha>

CI result summarizes its dependency results. Its success proves the event's
required graph passed; packaging additionally requires the exact-SHA receipt
with every Rust oracle and retained packaging. Release push jobs explicitly check
out the branch head SHA; identify the producing push run by head_sha. Never borrow green evidence from a parent,
equal tree, different attempt or different commit.

Runner capacity switches (owner operation):

| Repository variable | Value | Effect |
| --- | --- | --- |
| CI_LINUX_LIGHT_RUNNER | ubuntu-24.04 | Current private light labels per job |
| CI_LINUX_RUST_COMPILE_RUNNER | unset or `private` | Clippy and CLI tests on hosted ubuntu-24.04 by default; `private` routes them to the ubunta linux-build runner |
| CI_LINUX_RUST_FULL_RUNNER | unset or `hosted` | Full Rust suite on the ubunta linux-build runner by default; `hosted` routes it to ubuntu-24.04 |
| CI_RELEASE_EVIDENCE_RUNNER | unset or `hosted` | Release evidence promotion; master always uses hosted ubuntu-24.04, with metadata-only admission and retained-byte fallback |
| CI_MACOS_RUST_RUNNER | macos-15 | Air macOS platform tests |
| CI_MACOS_EMBEDDINGS_RUNNER | macos-15 | Air embeddings after platform tests |

Set only the literal hosted values listed above after repository visibility,
Actions budgets and one authorized capacity qualification are ready. Leave a
variable unset to retain the current private labels. No variable is changed by
this patch. Hosted variants receive no repository/environment secrets, use the
GitHub Actions sccache backend and cache only Cargo registry/git dependencies.
They retain artifacts in GitHub rather than attempting private S3. Private
runners keep their existing cache and receipt paths. The hosted macOS capacity
must be measured for embeddings before enabling it; signing and notarization
always remain on the private Macs. Pure TS/plugin jobs already use hosted
Ubuntu.

Source-repository selection uses GITHUB_REPOSITORY in Actions and the checkout's
origin for local scripts; DOMIOS_SOURCE_REPOSITORY is the explicit local override.
Historical issue declarations use one reviewed alias list in
scripts/source-repository-aliases.json and normalize to the runtime repository.
New declarations and API routes always use that runtime name.

For a plugin release, use `release/plugin-<id>` from current master and open the
owner PR directly into master. Scope is evaluated against the exact current
master commit and includes deletions and both sides of renames. Only the named
plugin's directory, its sole versioned channel entry and explicitly assigned
shared fixtures can qualify. Mixed changes require a full `release/next` push
and PR. After the plugin PR merges, real-merge master into develop, then tag
the master merge and dispatch `plugin-release.yml` on master with the exact tag.
Both publication jobs still require the `release` environment.

## macOS x64 compilation split: 1.15.4 design

For 1.15.3, macOS x64 compilation, signing and notarization stay on the existing
private Mac job. The server embeds the exact SHA-256 of the signed worker CLI
as SERVER_CLI_SHA256 and checks it when pinning that executable. Signing the CLI
after compiling the server changes its bytes and invalidates that pin. Final
macOS resource staging also verifies the server's approved Developer ID team.
An unsigned hosted build followed by private signing cannot preserve these
contracts as a two-phase graph.

The 1.15.4 design preserves the pin through four phases:

1. A hosted Intel Mac compiles the CLI and freezes an unsigned digest handoff.
2. A private Mac verifies that handoff, signs only the CLI, and returns its exact
   signed bytes in a second digest-pinned artifact.
3. A hosted Intel Mac verifies the signed CLI artifact, then compiles the server
   and client against those bytes. The server's compiled CLI digest must equal
   the signed artifact's SHA-256. It freezes the raw binaries, build/closure
   receipts and resources in another digest-verified handoff.
4. The private Mac verifies that handoff, signs the server and app, creates the
   DMG without compiling, notarizes/staples, and finalizes the full/headless pair.

Hosted phases receive only the six approved public-client configuration secrets;
signing, notarization, updater and publication credentials remain private.
Every handoff binds its inventory to the target, source SHA, tag, producing run
and attempt. The finalization proof must distinguish approved signing changes
from unexpected byte changes, retain the signed CLI digest unchanged, and prove
the full and headless packages contain the same final server/CLI pair.

Use the hosted Intel label macos-15-intel when implementing the bridge, after
rechecking the [official runner image list](https://github.com/actions/runner-images).
This design changes neither the runtime CLI pin nor its trust policy.

For a runner-only diagnostic, dispatch the existing CI workflow with
scope=preflight and an exact source_ref. For example, from the source checkout:
gh workflow run tests.yml --ref release/next -f source_ref=<full-40-hex-sha> -f scope=fast.
This probes hosted Linux/Windows/macOS plus Ubuntu light/build services, Mac and Windows
self-hosted classes. It installs tools only on hosted runners, compiles nothing,
runs no suites and writes no qualification receipt. The CI diagnostic result still reports
a failed or cancelled probe. The workflow revision must be pushed before dispatch.

Python setup resolves python3 on Linux/macOS or python on Windows and exports
the exact job-local interpreter as CI_PYTHON. Both pip and Python test runners
use that interpreter; no system pip install or python alias is required.
Catalogue selection runs during triage even after a previous failure. Commands
run through ci-catalogue-command.mjs, which reports a named catalogue error for
a missing/invalid selection instead of failing workflow expression expansion.

Guards always run on secret-free ubuntu-24.04. The private full Rust suite,
Clippy and CLI use the same host-ubunta linux-build executor; the Rust tier
starts after the fast tier, preserving one compilation per host. BLACKSTAR WSL
is retired from workflow routing. Windows remains on ROG and macOS on the Macs.

## Hosted switch prerequisites

Linux compilation is hosted by default; signing stays private. Before moving more
work to hosted runners:

- Install Linux GUI development dependencies (WebKitGTK 4.1, GTK, appindicator,
  OpenSSL, librsvg, xdo and bundle utilities) before workspace compilation.
- Derive hosted mode from runner capability, rather than matching one label;
  validate the selected OS and architecture, secret suppression and cache backend.
- Replace release self-hosted label arrays with a complete hosted bootstrap design.
  Signing and notarization stay on private runners.
- Supply explicit public fixture client configuration for secret-free test builds,
  skip local secret lookup in CI and distinguish that coverage from configured releases.

Small CI evidence (Rust receipts/warnings and frontend bundles) uses GitHub
artifacts on every Actions runner, including private Rust producers. Large release
bundles and signed evidence retain the private-store/publication path. Spools and
CI JSON expire after three days; frontend and unsigned ARM64 transfers after one day.
Readers merge every spool and reject differing copies of one key.
Frontend bundles and unsigned ARM64 handoffs are addressed by the producer's
SHA-256, so retrying a consumer does not change which bytes it receives.
The six public client configuration secrets remain repository-level inputs to
the environment-less hosted ARM64 compile; signing credentials stay private.


For a fast diagnostic of a commit that is already on master, develop or the release
branch, dispatch from the release branch with the full 40-hex SHA:

gh workflow run tests.yml --ref release/next -f source_ref=<full-40-hex-sha> -f scope=fast

Admission refuses any other dispatching ref, an abbreviated or symbolic source_ref, and a
commit not reachable from one of those owner-only branches: diagnostics run with farm
secrets, so a contributor's PR head is never eligible. The runner-group allowlist admits
the release ref only, so a scratch branch could not acquire the farm either.

This runs guards, TS static/unit/Vitest, frontend/path identity, generated types,
plugin bundles/tests and CLI manifest/parser contracts. It preserves the existing
runner and secret boundaries. Clippy, Rust full/platform/embeddings, warning
aggregation and qualification receipt are skipped. CI diagnostic result explicitly
labels the diagnostic scope and rejects an unexpected qualification job; this run
cannot admit release packaging or a master merge.
