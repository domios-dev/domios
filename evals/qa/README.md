# Curated live QA catalogue

Proposed repository home: `evals/qa/`. These are repeatable operator journeys, not a new automatic build or CI gate. The testing skill owns test-layer policy; these recipes name exact live oracles. No recipe authorizes a restart, production mutation, remote pairing, reset consumption or external message by itself.

## Release core

Run this bounded subset once on the final release candidate under the Orchestrator's serialized QA grant. Record each row independently. A required NOT RUN leaves release acceptance incomplete. Sign-in is mandatory before release; a local-provider login, backend fixture or a synthetic challenge cannot replace it.

| Journey | Required core slice | Extra scope needs a separate grant |
|---|---|---|
| [sign-in](sign-in.md) | Installed Firebase: genuine app-generated challenge, password sign-in, same UID, account UI, sign-out; current supported negatives | Owner restart for saved-credential recovery; production account surface only |
| [chat-send](chat-send.md) | Owned Codex runtime and Claude PTY: one submitted input, one own-session turn, final answer and one visible bubble; Stop stays interrupted after view reload | Heavy turns, memory pressure, provider subagents |
| [images](images.md) | PNG/JPEG reach own session exactly once and model describes an opaque image fact; pending/failure does not duplicate text | Physical iOS/HEIC/pickers and injected HTTP failures |
| [browser](browser.md) | Real form, state flags, stale refs, explicit event wait and deadline, clean profile restrictions; selection through viewer | Held-network, iframe and popup adversarial slices |
| [Codex usage/reset](codex-usage-reset.md) | Raw own-runtime balance matches Account credits; reset count/expiry separate; open confirmation, CANCEL, zero consume requests | No recipe permits confirming a reset |
| [worker kinds](worker-kinds.md) | One dispatched Worker ACK/report and one task-less tester report; unrelated review refused, handed review accepted/bounced with far-end evidence | Hostile sandbox probes and expanded role matrix |
| [restart/restore](restart-restore.md) | Owner restart: bound Worker keeps task/attempt/session and can report; idle retained tabs remain frozen until visited/input | Busy shutdown and upgrade cycle |
| [Telegram](telegram.md) | Owner-authorized inbound and reply: exact recipient/thread and one persisted answer | Bot/poller transfer, topic deletion, main-chat production smoke |
| [mesh/jobs](mesh-and-jobs.md) | Local manual and scheduled job persisted result plus Jobs UI; mesh read-only peer inventory when configured | Pairing/restarts/remote mutation and timing parity |

The owner must explicitly scope an unavailable optional integration out of a particular release; do not silently relabel NOT RUN as PASS. Keep the core small, and add a case when a concrete escaped defect justifies its recurring cost. Broad suites and compilation remain separate, owner-run gates.

## Start and evidence

Read `.agents/skills/testing/SKILL.md`, `.agents/skills/dev-testing/SKILL.md`, `codeterm docs BROWSER` and each journey's owning docs. The current repository bench library overrides stale dev-testing connection advice. From Git Bash in canonical:

```bash
unset CODETERM_CLI_PATH
source evals/_lib/lib.sh
source evals/qa/tools/dev-session.sh
qa_run health dev_get /api/health
qa_run topology dc tab list
```

Set `QA_OUT` to a new run directory before sourcing the logger. Every DEV command is `dc`, `dev_get` or `dev_run`; never inherit a host/prod caller. Use `dc --as-tab "$OWNER"` and `dev_run --as-tab "$OWNER"` only after recording a real, owned DEV agent. Host tab IDs are not DEV identities. A task-less tester may create fixture agents only with the parent's explicit grant. New agent titles are one word and `--description` is required. Bodies live in the caller's pad and use `--task-file`/`--from-pad`; never interpolate them into shell arguments.

Before action, pin expected health build, boot ID, port and provider/plugin provenance. A restored process or a source manifest is not proof of the active plugin version. Keep an ownership manifest of workspace, Team, tabs, sessions, binding nonce, jobs, Browser targets, processes and files. Capture state before any tab visit that might thaw it.

Each evidence row includes: journey/subcase; UTC time; build/boot; fixture/tab/session/thread/turn/send/run IDs; exact action once; raw exit/HTTP status; relevant redacted JSON; DOM fact; screenshot absolute path actually opened; verdict; remaining scope; cleanup. A request/ACK/200 alone never proves its far end. Record the real response schema; do not infer user-turn counts from a screenshot or echoed prompt.

PASS means every required subcase has fresh far-end proof. FAIL means an executed mismatch. NOT RUN means no valid execution, including missing account, capability, physical device or lifecycle grant. A failed test driver is NOT VERIFIED until its input/target is proven. A later repair needs a new run; preserve the old failure. Source facts, executed results and hypotheses are separate fields.

No sleeps, settle delays, polling intervals or blind retries. Wait on own-session events, process exit, DOM mutation, animation completion or acknowledged protocol state under a failure deadline. A timeout is failure, never readiness. Never replay held/written/uncertain sends or callbacks.

## Secret and production boundary

The generic tools in this draft refuse production. Sign-in is a separately authorized installed-production recipe: port 7685 is permitted only for that account grant, and prod is never killed. Secret-store values, tokens, challenge/verifier/state/code and email/UID remain in process memory. Do not capture credentials in argv, logs, pads, screenshot chrome, page titles or reports. Filter both the URL and its duplicated page-title prefix. Report UID equality as `same_uid=true`, never the UID. Avoid logging secret-store get stdout.

## Provenance and current limits

Sources read: Elgar's copied `turing-ledger.md` and Keel's `tester-handoff.md` under his host pad, plus Joy's executed checks. Turing's original pad was deleted; the copied ledger is a summary, not raw per-step evidence. Historical PASSes below seed recipes and never satisfy a future release run.

Joy's DEV run: build a73199a03, boot95122e98c4544689af68c465060c17d6; owned workspace f661d088, Codex31db53e0/session01a14a91-d472-7c41-8930-21dfaf26ac5d, Browser19cb9a31, serverfee60b0a. All were cleaned; server PID35656 exited and the empty provisioned directory was removed. Screenshots/results were copied to Joy's host pad before cleanup.

Tools here are generalized drafts of the used helpers. Syntax checks are not live revalidation of generalized parameters. See [tools](tools/README.md). Existing `evals/core-journeys` is a separate, restart-capable named-instance bench; its authorization and ports apply only to its own run. Older restore docs mentioning Playwright do not override the current suspension.
