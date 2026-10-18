# Worker kinds, reports and handed review

Purpose: prove task authority, runtime identity and reporting reach the parent; core is one ordinary dispatch and one task-less tester lifecycle. Only the owner can grant fixture hierarchy and reviewed-task delegation.

## Preconditions / fixture ownership

Owner provides a disposable DEV Team/Plan, its real Orchestrator, two tiny tasks, owned tester and Worker identities, and exact allowed commands. Do not change a real project task. Read ORCHESTRATION and current `dc agent spawn --help`, `dc plan task dispatch --help`, `dc plan review delegate --help`. Use pad-authored task bodies. Testing leaf agents never spawn a worker team without this explicit fixture grant.

## Steps / far-end proof

1. Owner creates a tiny complete task in the disposable plan using the current required body, criteria, executor-role and role-guidance contract; dispatches it through `dc --as-tab "$ORCH" plan task dispatch "$PLAN" "$TASK" --execution-intent read_only`. Preserve returned tab/attempt/lane and assigned gate tokens. Wait on exact launch/ACK events, not text matching readiness.
2. In the actual Worker session, `dc --as-tab "$WORKER" agent ack`; task must move to in_progress for its current attempt, not somebody else's. Its minimal authorized work writes a report in its own pad, then `dc --as-tab "$WORKER" agent report --status in_review --from-pad report.md`. Parent readback must include exact sender/session/body/report ID; task in_review with correct attempt. No fictitious commit SHA for read-only work.
3. For a task-less tester, owner uses `agent spawn ... --kind tester` under its current allowed task-less form and explicit scope. Verify tester-kind prompt/lifecycle and absence of task binding from actual metadata. In that tester session, report with `agent report --from-pad report.md` and NO --task/--status; prove the parent received it. `structurally_not_attachable` can still be a successfully delivered hierarchy-only report; verify accepted=true/state=delivered.
4. Have the tester attempt accept/bounce only an owned unrelated disposable task: it must refuse as not handed and leave task/attempt unchanged. Then owner executes `dc --as-tab "$ORCH" plan review delegate "$PLAN" "$TASK" --tester-tab "$TESTER"`. The tester independently runs every acceptance criterion, not merely reads the author report; it may accept all-pass or bounce with concrete failing evidence using current review verbs and a pad-authored note where supported. Verify durable review note and resulting task/author delivery. No authority inferred from Team membership.
5. Access variants are extended cases: ordinary Worker defaults to full access; explicit `--sandbox read-only` confines it. Use only an owned temporary file and a tiny command, record actual write/read/remove result and raw denial. Do not treat execution_intent=read_only as OS sandboxing. No install, network or broad shell probe without exact grant.
6. If model/restart is in scope, record task/attempt/session before and after `dc agent model ...` or `dc agent restart --tab ...` discovered from help; prove actual Worker report after the control succeeds with preserved binding. Metadata alone does not prove it can work again.

PASS: exact assigned ACK/report and far-end parent state, tester no-task report, refused unrelated review and explicitly handed independent review. FAIL: false success, missing report, role mismatch, escaped task authority, stale attempt or report refusal. NOT RUN: missing owner/delegation/command grant. A coordination failure is a product incident; route it immediately to the owner, preserve uncertain delivery and never resend blindly.

Cleanup only disposable tasks/Team/workspace after owner review and evidence; preserve real merge ancestry for any lane. Do not mark implementation tasks done yourself without handed review.

Seeds: Turing's consolidated ledger reports ordinary/task-less/task-bound QA ACK/report, sandbox distinction and review delegation PASS. Keel earlier lacked final report-after-model-switch proof. Each new core run must collect fresh exact report IDs. Existing `evals/plan-authorized-reporting`, `evals/worker-handoff` and runtime-provider matrix contain mechanism benches; their own execution/lifecycle grants still apply.

Automation: scoped report/authority models and one real dispatch/parent-delivery oracle. Avoid tests that assert prompt wording; test composed role fragments as opaque tokens and actual own-session delivery.
