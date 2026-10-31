# Repeatable curated QA

Read this before running a curated case, after `README.md` and before the selected `case.md`. This procedure belongs to the tests, not AGENTS.md.

## Four test families

- Curated journeys prove real owner actions in a real Browser/application and, when needed, real agents. Repeatable mechanics can be scripted. QA still inspects the actual result and makes the case verdict. Archived Playwright scenarios are a source of reusable actions and oracles, not a second active catalogue.
- Unit and deterministic contract tests prove decisions without real models. API/integration checks are labelled explicitly under this automated family; they do not silently become curated live work.
- Load/performance tests measure latency, throughput, resource growth and stability under an explicit workload. They retain workload, environment and measurements separately from functional verdicts.
- Agent evals measure task behavior/quality. They sit operationally beside load tests: opt-in execution, model/data/version pins, token/time budget, metrics and analysis. One model task can be an eval without being a load test.

The [testing skill](../../.agents/skills/testing/SKILL.md) owns runner/layer policy; [family navigation](../README.md) leads to the maintained procedures. The runner index remains `tests/catalogue.json`. Curated case identity and result matrix remain `evals/qa/catalogue.json` and its existing result schema. Do not create a competing list of live cases. Folder name `evals/` alone does not classify a test.

## Record actions, not a fragile click recording

A reusable segment declares its CUR case/subcase, target identity, prerequisites, resources it owns, semantic actions, final oracle and cleanup. Use the built-in Browser CLI. Use accessible role/name, a fresh snapshot and explicit DOM/event waits. Do not store old DOM reference IDs, coordinates, delays, copied tokens or a browser profile as proof of identity. Stop on missing/ambiguous/changed controls rather than clicking an approximate substitute.

Good scripted work: open a verified origin, select a known owned tab, inspect form flags, fill a composer, click once, wait on a result, read the same bound transcript, correlate IDs, verify persistence, collect bounded redacted evidence and close owned fixtures. Human/agent judgment remains for visual correctness, a new workflow decision and the meaning of a natural response.

Never compare an agent's answer wording. For transport, prove a fresh exact own-session user append and a subsequent assistant turn after it, on the same binding and completed turn. For Neighbor communication, additionally require the actual recipient's incoming message ID and the reply's matching in_reply_to at the sender. A generic assistant answer is not proof that an external action succeeded.

## Once-only actions and fast continuation

Read-only setup can be repeated after fresh identity checks. Writes have one operation identity and a fresh run ledger. Record the attempt before dispatch; preserve the returned result separately. After an uncertain send/click, observe the existing operation. Never retry, reload-and-send, reset the fixture or restart to make an uncertain write disappear.

Keep reusable preparation separate from irreversible scenario steps. A saved stage is not permission to skip current build/boot/account/session/revision checks. Resume from observed retained state and only execute a new distinct authorized action. Grants, first receiver assignment and Apply are not generic setup to repeat every run.

## Final oracles

A visible control, request ACK, exit 0 or HTTP 200 is an intermediate observation. Name the far end: the delivered own-session item, returned form value, persistent row/history, linked Neighbor reply, rendered owned tab or exact process absence. A successful scripted segment does not make the full CUR case PASS. Keep unexecuted subcases NOT RUN and model/security/load claims separate.

## Moving archived Playwright coverage

1. Identify the user behavior and existing CUR case first. Record the source spec and useful oracle.
2. Revalidate selectors, endpoint, provider and fixture authority against current source/UI. Do not carry over native-CDP-only assumptions, retired routes, fake role binding, old models, arbitrary resets or timed waits.
3. Extract semantic action segments and pure result checks. Reuse current core-journey own-session/event/model-policy helpers where they fit; do not copy their obsolete assumptions into a new runner.
4. Run a focused pure negative test and an owned Browser segment. Retain raw failures. Source migration is not live coverage.
5. Link the segment and migration disposition in the existing case. Leave `automation: manual` until the required case/subcase scopes are accurately represented; marking a partial helper as full scripted coverage is wrong.
6. Propose archive retirement separately after its replacement is reviewed and registered; this integration deletes or reactivates nothing. Keep stable CUR IDs and historical evidence. Do not re-enable the suspended Playwright runner merely because its assertions were reused.

## Controlled-form subset

[`tools/browser-steps.mjs`](tools/browser-steps.mjs) exports `runSteps({origin, steps, out, call?})`. It is deliberately limited to an owned isolated HTTP fixture at `127.0.0.1` with an explicit port other than 7685/7686. The caller verifies ownership of that port and Browser before use; this is not a sandbox against a redirect or a changed page. It does not authenticate or navigate the product, allocate a fixture, open/close Browser or grant authority for another port. Use current `domios docs BROWSER` for product workflows; there is no blanket native CDP prerequisite.

Open your owned clean Browser, serve a form fixture on an OS-assigned loopback port, and use a new absolute `out` directory inside your pad. Supply 1–30 uniquely named steps. The supported argv forms are `goto [same-origin-url]`, `snapshot []` or `[line-limit]`, `type [role, name, synthetic-value]`, `click [role, name]`, and `wait ['--role', role, '--name', name, '--state', state, '--timeout', '15s']`. Role/name must come from a fresh snapshot; the Browser's ordinary role/name lookup can use substring matching, so inspect exact names and stop on ambiguity. CSS selectors, stored refs, cross-owner flags and timing waits are refused. This helper handles non-secret synthetic fixtures only; it saves raw command output and cannot redact arbitrary page secrets.

For a form named like Sage's fixture, the selected segment is:

```js
const steps = [
  {id: 'open', verb: 'goto', args: [origin + '/']},
  {id: 'form-state', verb: 'snapshot', args: ['80'], expect: 'Required name'},
  {id: 'fill', verb: 'type', args: ['textbox', 'Required name', value], once: true},
  {id: 'filled', verb: 'snapshot', args: ['80'], expect: value},
  {id: 'submit', verb: 'click', args: ['button', 'Submit fixture'], once: true},
  {id: 'completed', verb: 'wait', args: ['--role', 'heading', '--name', 'Fixture submitted', '--state', 'visible', '--timeout', '15s']},
  {id: 'returned-value', verb: 'snapshot', args: ['80'], expect: value},
];
await runSteps({origin, steps, out});
```

Import `runSteps` from the helper; `origin`, `value` and `out` are owned fixture inputs. The fixture must record requests and return the submitted value in its result page. Require exactly one submission request with that exact synthetic value at the server as well as the Browser result. Inspect actual disabled/readonly/required/selected flags separately; a name substring assertion does not prove those flags. Missing flags, public-site navigation and nonexistent persistent-profile refusal remain unexecuted obligations unless independently observed. Close only your Browser and server, confirming fixture shutdown.

The helper writes an exclusive attempt file before each dispatch, saves raw results/failures and stops at the first error. Nonzero exits, missing expected state and output overflow never trigger retries; mutation failures are `action_unconfirmed`. An existing `out` refuses before dispatch. Creating a different output directory does not authorize repeating an earlier uncertain operation. `completed.json` means selected steps completed, without a case verdict. `expect` is only for deterministic fixture state, never model prose.

## Own-session composer and linked-reply segment

1. Verify current target/build/boot, intended tab, provider capability, claimed session ID and binding nonce. Capture the ordered transcript ID prefix before the operation. If the session is not yet bound or no trustworthy append boundary exists, stop; this helper does not implement lazy-session first-send admission.
2. Take a fresh Browser snapshot, resolve the owned composer by its current accessible role/name (it may be a combobox), fill one opaque synthetic payload and submit once. Retain operation ID and result. Uncertainty means observe that same operation, without replay, reload-and-send or fixture reset.
3. Read the exact same bound session through its supported transcript/event interface. `ownTurnVerdict` accepts `session`, `actualSession`, `nonce`, `reportedNonce`, `beforeIds`, ordered full `messages` (`id`, `type`, `content`), `payload`, and `completedReplyIds`. Adapters supply completion IDs only from authoritative completion events for that same session and nonce. Never derive them from `idle`, visible prose or a generic ACK. An adapter without such evidence leaves completion unconfirmed.
4. The pure oracle requires an unchanged ordered prefix, unique message IDs and exactly one new exact user payload. It returns `delivered` for append alone and `roundtrip_observed` only for a nonempty subsequent assistant reply with explicit completion before the next user boundary. Wrong binding/window is `refused`; missing/duplicate input is `unconfirmed`. This models a stable ID transcript; it does not replace the product's byte/cursor-based delivery ledger. Prove one rendered row per source ID and persistence separately in CUR-001.
5. For CUR-048 additionally correlate actual recipient incoming message ID, admitted own-session turn and relay ACK/deletion, then require the sender's incoming reply to carry the matching `in_reply_to`. A local assistant answer alone cannot prove an encrypted reply. Keep receiver/grants/friendship as retained prerequisites; setup changes require distinct authority. Keep each failed direction and diagnostic distinct; never resend a failed original to produce green evidence.

## Retained evidence and archive migration candidates

Sage's prototype and controlled-form evidence remain in the original QA pad. That form demonstration covers only its synthetic fixture, not Domios behavior or a complete curated case. Product verdicts require executing the selected scenario against the actual application and observing its far end.

The prototype's `migration-inventory.json` and `CLEANUP-AUDIT.md` are advisory source inventories, not coverage verdicts. Revalidate each candidate against the current case and UI before any migration:

| Archived behavior | Candidate existing case | Retain/revalidate |
|---|---|---|
| Composer draft isolation/focus | CUR-029 | Owned tab drafts, switch/reload, current combobox role |
| Fresh-tab send/chat-send matrix | CUR-001 | Exact own-session append, IDs/render count, no wording oracle |
| Quiet session/restore matrix | CUR-011/026/027 | Same binding, persistence, no replay; independent restart grant |
| Model switch/new-session picker | CUR-039/040/041 | Current catalogue/requested/observed model/settings equality |
| Mobile overflow/sidebar/canvas header | CUR-003/030/031 | Accessible actions plus actual visual inspection |
| Image paste/tool cards | CUR-004/005 | Uploaded bytes/intended session; pure presentation logic stays unit-tested |
| Remote-tab rendering | CUR-033/034 | Actual owned remote far end; no pairing implied |
| Smoke/multiplexer/settings | CUR-032/035/036 where applicable | Map behaviors individually; unmatched scenarios remain unmapped |

The retained Neighbor resume16 exchange is a candidate fixture profile, not a new result here: its reported B-send/A-receive success and A-local-reply failure remain distinct. Retained account/grant/receiver state is not replayable setup. An identify-only diagnostic is a distinct operation, never permission to resend an old question. No schema, archive runner or legacy lifecycle helper change is part of this integration.

## Entry and exit

Run only on the assigned target; a case is not authority to spend model tokens, sign in, mutate grants, restart shared DEV or send externally. Pin source/build/boot and ownership before action. Use `evals/_lib/lib.sh` for fixture CLI/API actions; Browser management keeps its own host connection. Secrets remain in supported private boundaries, never test argv, recordings or reports.

Every run records case/subcase, action IDs, current target/session, timing, raw outcome, far-end evidence, first failure and exact cleanup. Use existing PASS/FAIL/NOT RUN/BLOCKED/OWNER-SKIPPED meanings. On failure stop at the first new boundary; do not repair product policy inside the test. Close only owned resources and preserve retained fixtures when the owner requested it.
