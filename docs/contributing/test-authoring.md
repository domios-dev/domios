# Authoring automated tests — the contract

How to write tests that are harder than unit tests: Playwright specs, live
drills, eval harnesses, anything where the system under test includes a
daemon, a UI, or an agent. Written after a release cycle in which every
class of mistake below shipped at least once. Generalized: nothing here is
specific to one suite.

This doc is the operating instruction for the **test-author** role. If your
task's deliverable is primarily tests, this is your charter — read it fully
before writing the first line.

## The one law

**A test that has never failed is not a test.** You do not know what it
proves until you have watched it catch the defect it exists for. Every rule
below serves this law.

## 1. Oracle first

Before any code: write one sentence — *what observable fact proves the
behavior, and how does the test read that fact?* If you cannot name the
oracle, you are not ready to write the test. "The page looks right" is not
an oracle; "the board card for task X renders chip 'pane gone' within 2s of
the pane dying" is.

Order-sensitive systems get **order oracles**: assert the sequence of
transcript/log markers (`s1 < compact < reorient < resumed`), not just the
final state. A correct end state reached in the wrong order is a latent bug.

## 2. Red before green

Prove the test can fail. Run it against sabotaged code (comment out the fix,
break the layout, skip the phase) and watch it go red for the *right reason*.
Then restore and watch it go green. A red-green demo belongs in the report of
every new test. Case study: an ordered-transcript drill was written, never
executed, and shipped with a CLI signature error on line one of its setup —
it could not have run even once. The author believed it worked.

## 3. Never sleep — wait on conditions

Fixed sleeps encode a guess about timing; the guess is wrong on the next
machine, under load, or after an optimization. Always wait on the condition
itself, bounded:

- UI: `expect.poll`, `waitForSelector`, event assertions — never
  `waitForTimeout`.
- Shell/drill: poll for the log marker / file / health endpoint with a
  timeout loop (`until <check> || deadline; sleep 1`).
- Agents: **spawned is not ready.** A pane existing does not mean the agent
  accepts input. Poll readiness (`tab status`, the launch-ready log line)
  before the first injection. Case study: a drill triggered its scenario 8s
  after spawn; the agent took 15s to become ready; every injection bounced
  and the test "hung" — the race was in the harness, not the product.

Justified sleep residue (a truly time-based contract) carries an inline
one-line reason, or it gets deleted by the next hygiene pass.

## 4. Playwright mechanics

The Playwright suite has concrete helper contracts; use them instead of
re-deriving timing and cleanup in each spec.

- Canvas and Mission Control specs wait on `waitForCanvasSettled` from
  `tests/playwright/helpers/canvas-inspect.ts`. It watches projected geometry
  across animation frames, so a spec waits for the app surface instead of a
  blanket sleep.
- Daemon-killing coverage is isolated in the restart project. Specs tagged
  `@restart` are excluded from shared `smoke`, `golden`, and `full` runs, then
  executed serially so ordinary shards do not lose their daemon mid-test.
- The default fixture snapshots state with `snapshotBaseline` and tears down
  only test-created work with `diffReset` from `tests/playwright/helpers/reset.ts`.
  Fixture setup may self-heal a dead dev daemon, but one crash must fail one
  test, not cascade into a whole shard.
- Agent-flow golden tests use the debug-only `CODETERM_STUB_AGENT_CMD` transport
  when real provider liveness is not the oracle. The replayer lives under
  `tests/playwright/fixtures/stub-agent/` and preserves the real provider launch,
  parser, detection, status, and session paths.
- Playwright measurement harnesses live outside flow assertions. Latency, rAF,
  longtask, and replay-speed probes produce metrics artifacts; product-flow
  specs assert behavior and keep their own caps tight.

## 5. Testing systems that contain agents

The hardest class. Rules:

- **A bare agent is not a role.** A freshly spawned pane with no prompt will
  not write handoffs, resume work, or answer injections meaningfully. Either
  charge the test agent with a *minimal role contract* (the few lines that
  make it respond to what the test exercises) or use a structural fake at
  the seam below the agent. Decide which — never test against an agent that
  can neither pass nor fail meaningfully.
- **Failure branches are first-class scenarios**, not afterthoughts: the
  hung phase, the dead resumption, the timeout → alert path. In agentic
  systems the failure branch usually IS the safety property.
- **Determinism over liveness where timing doesn't matter**: seeded
  fixtures (known plan, known pane tree, canned transcript) beat live setup
  — faster, exact oracles, no flake. Go live only when liveness is the thing
  under test.

## 6. Bounded everything

Every run has a per-test timeout and a global timeout. Every spawned process
group can be killed as a group. A test that *can* hang forever is a defect
regardless of what it asserts. When a cap fires mid-execution, that is a
finding to read, not a number to raise.

## 7. Scripts are code

A drill/harness that shells out to a CLI must be smoke-run against the real
binary before it is committed — CLI signatures drift, hint lines pollute
stdout, `--json` output gets wrapped. Parse defensively (extract the JSON,
don't assume the stream is clean). The harness gets the same red-green
treatment as the test.

## 8. Say what you test

Every spec/drill opens with a 3–8 line header: **WHAT** is tested, **WHY**
it exists (feature contract or regression root cause), and the **ORACLE** in
one sentence. In-file codename taxonomies (RAIL-4, GRP-*) are defined in the
header that uses them or not used at all. Headerless test files fail review.

## 9. Evidence, not claims

A test report states: the exact command run, pass/fail/skip counts, and for
reds — the trace/log excerpt naming the failing oracle. "Passed locally" with
no command is not evidence. Skipped suites are listed with reasons; silent
skips read as coverage that does not exist.

## The test-author role

- **When assigned**: the Planner sets `executor_role: test-author` on any
  task whose primary deliverable is tests (new specs, drills, eval
  harnesses, baseline suites) — decided at task creation, like every role.
- **How the charter arrives**: the task's `role_guidance` names this doc as
  the operating instruction ("You are a test-author; docs/contributing/
  test-authoring.md is your charter — read it before writing"), and the
  dispatch brief carries it. The role never depends on the agent
  remembering to look.
- **Interim dispatch rule**: until task dispatch is the only worker launch
  path in a deployment, the Planner pastes the canonical role_guidance
  template into any manual spawn brief for a test-author task.
- **Acceptance shape**: every test-author task's criteria include the
  red-green demo and the isolation-first run of each new test. A test-author
  report without a red-green demo bounces.

## Embedded docs decision

Ship this charter as `codeterm docs TEST-AUTHORING`, not only as a repo doc.
Rationale: it is tier-3 product-general operating knowledge for agents who
write tests in any project, and it passes the stranger's-repo check because
the rules name no CodeTerm-only internals beyond the CLI smoke-run examples.
Keeping the repo path in the role_guidance still matters for local contributors;
the embedded copy gives spawned agents a stable recovery path when they are
outside this checkout.

<!-- revision: 2 -->
