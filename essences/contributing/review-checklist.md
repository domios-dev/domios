# PR Review Checklist

**Reviewer: verify the following before approval.**

## Evidence & Testing
- [ ] Verification section contains real command output (not just claims)
- [ ] New/changed tests use fail-first method: bug fixes show red→green, sabotage tests fail without fix, refactors show all-green three times
- [ ] Tests assert the true user-journey end state, not just API/status responses
- [ ] Test layer justified (unit / integration / E2E) per `essences/testing.md` decision tree
- [ ] Test shape follows `essences/contributing/writing-tests.md` honesty and harness rules
- [ ] Test fixtures used, not raw setup; no hardcoded paths or magic numbers

## Documentation
- [ ] Revision tags bumped on every touched `.md` file
- [ ] File size check passes (no source file over 1200 lines; there is no exception)
- [ ] Generated artifacts (CLI manifest/reference/command paths, `essences/map.jsonl`) are regenerated from their source and committed with the change, never hand-edited; see `essences/build.md#generated-artifacts-and-the-embedded-doc-boundary`
- [ ] A document production agents read is registered in the embedded docs registry (or excluded with a reason); `prod-docs/` holds only embedded docs and everything else stays in `essences/`

## Code Quality
- [ ] Autonomous fixes are small, well-understood, low-risk, and one logical fix per commit; otherwise escalate options, trade-offs, and recommendation
- [ ] Sleeps/delays in tests are justified (not arbitrary waits)
- [ ] Cleanup paths clear temp files and restore state
- [ ] No comments beyond single-line WHY statements (not WHAT)
- [ ] Lock ordering respected if adding new Mutex usage

## Integration
- [ ] Changes respect cross-platform constraints (Windows paths, ConPTY, etc.)
- [ ] No breaking changes to public APIs without documented migration path

