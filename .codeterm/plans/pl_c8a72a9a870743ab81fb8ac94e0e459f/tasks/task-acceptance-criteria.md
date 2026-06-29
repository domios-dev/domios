# Every task MUST carry: acceptance CRITERIA + correctness + TEST PLAN (structured, required)
Andrey (hard requirement): a task without correctness criteria + a test plan is worthless — at acceptance (§38) the orchestrator has nothing to judge against and ends up guessing or asking the user. EVERY task must contain these elements.

## Data model (add to Task)
- `acceptance_criteria`: list of CONCRETE, checkable conditions that define done (e.g. "plan node CSS width identical at 100/200/400% zoom", "task body text crisp at 400%", "active tasks mean-x < done mean-x"). Each criterion should be objectively verifiable.
- `test_plan`: HOW each criterion is verified — unit test / Playwright spec (with the test name/grep) / manual oracle / a quick cursor-verifier. Prefer automated + named.
- `correctness`: one line — what "correct" means / the vector this serves (ties to the plan GOAL §34).

## Enforcement (CLI + orchestrator)
- `plan task add` must REQUIRE (or strongly guard, escalated from the ignored advisory hint) acceptance_criteria + test_plan — refuse/loudly warn a task created without them. `plan task add --criterion ... --test-plan ...` or an interactive/required body section.
- §38 acceptance: the orchestrator verifies the task's acceptance_criteria by RUNNING its test_plan before setting done. No criteria → cannot accept → the task is not properly specified.
- Canvas: show criteria + test_plan + pass/fail per criterion on the task card (the "results box" §37.1).

## Backfill
Existing open tasks lack criteria — add criteria + test_plan to the priority ones (worlds-v3 family first, then p0/p1).

## Why
This makes "done" objective and survives memory loss: a fresh orchestrator reads the criteria + runs the test_plan to judge, instead of guessing. Ties to §38 acceptance, §34 GOAL/vector, worlds-navigation (criteria visible on the card).
