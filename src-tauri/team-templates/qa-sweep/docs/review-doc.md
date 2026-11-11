# PR review doc

One review doc per pull request, written by the Worker that owns that PR. It is a Markdown file in the review folder agreed with the owner at the start of the sweep. Posting it to the pull request is an outward action and needs the owner's approval.

## Contents

- **PR**: number, title, author, head branch, base branch.
- **Reviewed SHA**: the exact head commit the review covers. A new commit on the PR means a new review round with a new SHA; earlier rounds stay in the doc.
- **Scope**: what the PR claims to change, and what the diff against the merge base actually changes.
- **Checks run**: each command or live step with its result, and the checks not run.
- **Findings**: one entry per finding, as below.
- **Verdict**: `no open Criticals` or `open Criticals: N`, plus a one-line recommendation (approve, approve after fixes, request changes).

## Findings

Each finding has an id, a severity, a location (`file:line` or the UI step), the evidence, the impact and a status.

Severity:
- **Critical**: wrong behaviour, data loss, a security or authority hole, a crash, a broken build or a failing required check. Blocks merge.
- **Major**: a real defect with a workaround, a missing check for an important failure mode, or a contract drift. Fix before merge unless the owner accepts it.
- **Minor**: maintainability or clarity problems with no behaviour impact.
- **Nit**: style only.

Status:
- `fixed` for a fix committed on the PR branch, with its commit SHA and the check that proves it.
- `proposed` when fixing is not authorized, with the suggested change.
- `open`, or `accepted by owner` with the owner's reason.

A finding is never deleted: a closed one keeps its entry and its new status.
