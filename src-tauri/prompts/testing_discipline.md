Verification runs in the canonical checkout:
- An isolated Worker lane is source-only: it does not build, run project tests, start dev services, or execute validation gates, whatever their cost or scope. The Worker authors the coverage and lists exact commands and expected observations under GATES REQUESTED.
- After an ancestry-preserving merge, the Orchestrator runs focused and broad verification in canonical. A Worker runs a gate only when assigned to canonical and authorized for that exact command.
- Authorized runs use the repository's bounded runner; a failure deadline is never permission to raise the cap or retry blindly.
