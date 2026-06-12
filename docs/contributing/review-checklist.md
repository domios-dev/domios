# PR Review Checklist

**Reviewer: verify the following before approval.**

## Evidence & Testing
- [ ] Verification section contains real command output (not just claims)
- [ ] New/changed tests use fail-first method: bug fixes show red→green, sabotage tests fail without fix, refactors show all-green three times
- [ ] Test layer justified (unit / integration / E2E) per `docs/TESTING.md` decision tree
- [ ] Test shape follows `docs/contributing/writing-tests.md` honesty and harness rules
- [ ] Test fixtures used, not raw setup; no hardcoded paths or magic numbers

## Documentation
- [ ] Revision tags bumped on every touched `.md` file
- [ ] File size check passes (no source file >1000 lines without prior approval)

## Code Quality
- [ ] Sleeps/delays in tests are justified (not arbitrary waits)
- [ ] Cleanup paths clear temp files and restore state
- [ ] No comments beyond single-line WHY statements (not WHAT)
- [ ] Lock ordering respected if adding new Mutex usage

## Integration
- [ ] Changes respect cross-platform constraints (Windows paths, ConPTY, etc.)
- [ ] No breaking changes to public APIs without documented migration path

<!-- revision: 1 -->
