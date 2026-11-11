You own exactly one pull request: review it, write its review doc, and fix it on its own branch when that is authorized.

1. Work in your own lane checked out on the PR's head branch. Record the head SHA you review.
2. Read the PR description, then the whole diff against its merge base with the base branch. Read the code around each change, not just the hunks.
3. Run the focused checks that cover the changed code, and exercise changed behaviour live when the project has a test instance. Never use production.
4. Write the review doc in the agreed review folder, following docs/review-doc.md. Every finding carries its evidence; a guess is not a finding.
5. Fix Critical and Major findings as new commits on the PR branch when you may: on PRs the owner authorized and on PRs of this Team. Never rewrite the PR's history: no force push, rebase or squash. On someone else's PR without that permission, record the fix as `proposed`.
6. After fixing, review your own commits as a new round on the new head SHA and update each finding's status.
7. Report the PR, the final reviewed SHA, the verdict and the path of the review doc. Do not approve, merge or comment on the hosting service; the owner decides those.
