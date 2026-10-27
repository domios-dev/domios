# CI and release admission

CI in tests.yml is the only validation push/PR pipeline. Its final **CI result** job is the
only required status on master and release branches. The ruleset configuration
is .github/admin/public-repo.sh; applying repository settings remains an owner
operation. Existing live rules do not change when that script is edited.

| Event | Checks |
| --- | --- |
| Owner push to release/X.Y.Z | Full CI on the PR head SHA, qualification, then signed packaging of all six platforms with retained dry-run artifacts in the same run |
| All PRs, including release PRs and forks | Hosted, read-only, secret-free guards; release heads run only light policy/body checks |
| Owner push to develop, rename/domios or master | Complete qualification unless develop carries a release push head SHA; that duplicate skips heavy work with a notice |
| Admin merges release PR into master | Verify the squashed master commit carries exactly the release head tree, then publish the retained bytes from that exact head's green push run through promote_run_id; never rebuild the merge commit |
| Nightly | Complete qualification of rename/domios and develop |
| Owner CI dispatch, scope=fast | All fast contracts against source_ref on their existing runner kinds; no Clippy, full/native Rust, embeddings or qualification |

The source guards finish successfully before either test branch can acquire compilers.
Shared guards cover sizes, provider boundary, crate layering, Tauri reach,
changed-file comments, API/prompt/eval documentation, secret scanning, issue
metadata and master accepting only owner release/X.Y.Z PRs. Each runner checks
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
nextest and platform matrices keep fail-fast off, and no failing qualification
can start packaging. A newer commit can still supersede an older run.
CI result reads named failing/timed-out child jobs from the Actions API; timeouts remain failures rather than cancelling siblings. Hosted TS checks have 15-minute bounds; generated types and the private guard role have 20 minutes.

The release branch push is the single full CI and packaging owner. Each push
runs the full suite on its exact SHA, then calls release.yml with dry_run=true.
Per-branch concurrency cancels the superseded run, including packaging. Its
CI result check satisfies the release PR head. Release PR events report
Release PR checks, so their light success cannot mask the push CI result. PR runs always remain hosted
and secret-free, including PR body checks; they never start heavy packaging.
Develop admission skips a SHA already owned by a release push run.

An admin squash-merges the green release PR (the only allowed method; the branch is deleted). The master push
event discovers the merged release PR, verifies the merge parents, selects the successful tests.yml push run of
the same release head, and sets promote_run_id. Qualification and promotion
validate the same-repository release/X.Y.Z push head, owner actors,
green jobs, exact SHA, producing run and attempt. All retained artifacts are
verified before publication; the master merge commit is never a build source.
The existing release environment and publication switch still protect shipping.
CI runs from the owner's release/X.Y.Z PR, so the trusted runner group serves this
repository's workflows without a ref list. A fork PR's run waits for owner approval,
fork PRs receive no secrets, and only the owner merges into master. Hosted unsigned
compile jobs receive only their existing public build configuration; signing
credentials remain in protected signing jobs. Branch synchronization after
publication is an admin-owned merge PR, never a workflow push to master/develop.

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
| CI_RELEASE_EVIDENCE_RUNNER | unset or `hosted` | Release evidence promotion on the ubunta linux-build runner by default; `hosted` routes it to ubuntu-24.04 over the public store edge |
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
gh workflow run tests.yml --ref release/1.15.3 -f source_ref=<sha> -f scope=preflight.
This probes hosted Linux/Windows/macOS plus Ubuntu light/build services, Mac and Windows
self-hosted classes. It installs tools only on hosted runners, compiles nothing,
runs no suites and writes no qualification receipt. The CI result still reports
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


For a fast diagnostic before moving the release head, push the workflow revision
to a scratch branch such as ci/try-preflight, then dispatch:

gh workflow run tests.yml --ref ci/try-preflight -f source_ref=<exact-sha> -f scope=fast

This runs guards, TS static/unit/Vitest, frontend/path identity, generated types,
plugin bundles/tests and CLI manifest/parser contracts. It preserves the existing
runner and secret boundaries. Clippy, Rust full/platform/embeddings, warning
aggregation and qualification receipt are skipped. CI result explicitly labels
the diagnostic scope and rejects an unexpected qualification job; this run
cannot admit release packaging. Dispatch is supported on any branch in the
source repository for the trusted owner.
