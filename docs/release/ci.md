# CI and release admission

CI in tests.yml is the only push/PR pipeline. Its final **CI result** job is the
only required status on master and release branches. The ruleset configuration
is .github/admin/public-repo.sh; applying repository settings remains an owner
operation. Existing live rules do not change when that script is edited.

| Event | Checks |
| --- | --- |
| Owner push to release/* | Shared guards, hosted frontend/TS/plugin jobs, protected Rust/CLI, native platforms, embeddings, warnings and exact-SHA qualification |
| PR, including release PRs | Shared guards and hosted, read-only, secret-free scoped frontend/plugin/CLI contracts; no protected runner or environment |
| Owner push to develop, rename/domios or master | Same complete qualification graph |
| Nightly | Complete qualification of rename/domios and develop |
| Owner CI dispatch | Guards and fast checks for source_ref; diagnostic evidence only |

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
scripts/requirements-ci.txt into their pinned Python interpreter and preflight
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

Release packaging uses a successful CI workflow_run for the same current release
head, or an owner dispatch that checks the same exact-SHA qualification receipt.
A failed qualification never requests packaging; superseded/deleted heads skip
with a notice. No red job is an expected wait.

GitHub reads workflow_run from the default branch. Until master carries this
workflow, dispatch a qualified release head explicitly:

    node scripts/dispatch-qualified-release.mjs <40-hex-sha> release/1.15.3

The helper verifies owner identity, qualification and the branch head twice,
then dispatches a dry run with tag pinned to that SHA and mirror=false.
Publication still needs owner GO and the release environment. Once the default
branch contains the trigger, a green release push starts packaging automatically.

Read status by commit, not by the latest run:

    source_repository=$(node scripts/source-repository.mjs)
    gh api repos/$source_repository/commits/<sha>/check-runs --jq '.check_runs[] | {name, status, conclusion, details_url}'
    gh run list --repo "$source_repository" --workflow tests.yml --commit <sha>
    node scripts/ci-qualification.mjs check <sha>

CI result summarizes its dependency results. Its success proves the event's
required graph passed; packaging additionally requires the exact-SHA receipt
with every Rust oracle. PR merge-ref checks describe that merge SHA; identify
the qualification push by head_sha. Never borrow green evidence from a parent,
equal tree, different attempt or different commit.

Runner capacity switches (owner operation; defaults stay unchanged):

| Repository variable | Hosted value | Default role |
| --- | --- | --- |
| CI_LINUX_LIGHT_RUNNER | ubuntu-24.04 | Current private light labels per job |
| CI_LINUX_RUST_COMPILE_RUNNER | ubuntu-24.04 | Ubuntu Clippy/CLI compiler |
| CI_LINUX_RUST_FULL_RUNNER | ubuntu-24.04 | BLACKSTAR Linux full suite |
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
Ubuntu, and Windows path identity uses windows-latest.

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
