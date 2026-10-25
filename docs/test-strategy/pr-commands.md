# Pull request command ownership

CI/tests.yml is the sole push and PR entrypoint. Its shared hosted policy job runs
source-size, boundary, layering, comment, documentation/eval, secret and issue
metadata guards on every change. tests-pr.yml is a reusable hosted, read-only,
secret-free graph; its scope resolver selects frontend, CLI and plugin checks.

tests/catalogue.json owns test commands and discovery. Behavioral contracts prove
each PR catalogue runner has an executing owner, selected change classes reach
their checks, failures preserve independent results, and live agents, daemon
fixtures and suspended Playwright cannot enter PR CI.

Trusted pushes run tests-fast.yml and tests-rust.yml, then warnings and exact-SHA
qualification. See [CI operation](../release/ci.md) for branch behavior, the single
required status and packaging admission.
