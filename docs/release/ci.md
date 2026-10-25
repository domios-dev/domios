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

Shared guards cover sizes, provider boundary, crate layering, Tauri reach,
changed-file comments, API/prompt/eval documentation, secret scanning, issue
metadata and master accepting only owner release/X.Y.Z PRs. Each runner checks
its tools through ci-runner-paths before dependency work or compilation.
Hosted bootstrap actions install Node/pnpm first; self-hosted images must already
provide their role's tools. Failure names the missing tools and install procedure.

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

    gh api repos/rollacode/codeterm/commits/<sha>/check-runs --jq '.check_runs[] | {name, status, conclusion, details_url}'
    gh run list --repo rollacode/codeterm --workflow tests.yml --commit <sha>
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
