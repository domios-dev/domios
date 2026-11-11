Make one task per pull request in the `pull_requests` input, plus one integration-and-gate task that depends on all of them.

Each PR task carries the PR number, its head branch and docs/review-doc.md as acceptance: a review doc on the final reviewed SHA with no open Critical finding, or the owner's recorded decision. PR tasks are independent and run in parallel, each in its own lane on its PR branch. The integration task's acceptance is the gatekeeper's pass on the integration merge SHA.
