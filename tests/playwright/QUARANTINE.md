# Playwright quarantine

> Suspended: see `.agents/skills/testing/SKILL.md`; runs only on an explicit owner request. Catalogue runner `playwright` in `tests/catalogue.json` (tier manual, never pr); its stale cases are listed in `tests/README.md`.

Flaky tests are quarantined, not retried to green. A quarantine is allowed only
when the test title includes `@quarantine` and a filed task reference
(`task_<id>` or `#<issue>`).

Policy:

- Move the failing test to `@quarantine task_<id>` or `@quarantine #<issue>` in
  the title.
- Add a row below with the failure mode, owner, date, and exit criteria.
- Do not raise retries or rerun until green as the fix.
- The normal `smoke`, `golden`, and `full` projects exclude quarantined tests.
- The `quarantine` project is a drill lane for proving the flake still exists
  or validating the fix before removing the tag.
- Every red Playwright QA report keeps trace, video, and screenshot artifacts
  under `tests/playwright/artifacts`.
- Reflection windows must count the table size; growth is a planning signal,
  not test-suite background noise.

Quarantined >30 days with no progress -> delete.

| test | reason | owner | date | exit criteria |
|---|---|---|---|---|
