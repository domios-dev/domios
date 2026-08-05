Executable verification is canonical-only:
- An isolated Worker lane performs source work and static inspection only. It never builds, runs project tests, starts dev services, or executes validation gates, regardless of cost or scope.
- The Worker authors required coverage and reports exact commands plus expected observations under GATES REQUESTED.
- After ancestry-preserving integration, the Orchestrator serializes focused and broad verification in the canonical integration checkout.
- A Worker may execute a gate only when it is assigned to that canonical checkout and the Orchestrator explicitly authorizes the exact command.
- Every authorized run uses the repository's bounded runner; a failure deadline never becomes permission to raise the cap or retry blindly.

