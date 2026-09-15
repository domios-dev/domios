## What

Brief description of the change (1-2 sentences).

For a PR targeting a non-default branch, declare each fixed source-repo issue on its own exact
`Repository-Issue:` line. The final `develop` → `master` PR converts these to `Closes #N`.
Declare shipped public issues separately. Remove examples that do not apply; do not use prose
issue references or GitHub closing keywords outside the final PR to `master`.

Repository-Issue: rollacode/codeterm#N
Public-Issue: rollacode/codeterm-releases#N

## Verification

Paste real command output showing the fix works. Claims without evidence don't count.

## New/changed tests

Describe the fail-first method used (bug-fix red→green, sabotage, or refactor triple-green) with evidence. Justify test layer — see `essences/TESTING.md` decision tree.

## Docs

Did you bump revision tags on every touched doc file? Does the file-size check pass?
