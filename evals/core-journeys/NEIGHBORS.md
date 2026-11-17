# J5 Neighbors

J5 proves the release-critical conversation across two disposable named DEV
instances: A sends one fresh question; B's host-created concierge answers with
its own scoped credential through the trusted reply CLI; A receives that exact
linked answer; both outgoing receipts become delivered and B returns to idle.
The runner repeats `read_only`, `folder` and `full`, retaining each grant's
enforcement note, concierge tab/session, question ID and answer ID. It never
creates a substitute agent or sends an answer as owner to repair the oracle.

Use [qa-peers](../../.agents/skills/qa-peers/SKILL.md) and
[CUR-048](../qa/cases/CUR-048-neighbors-encrypted-delivery/case.md) for peer
qualification and account setup. The build owner pins and qualifies native
attachment, frontend identity and boot evidence before this run. No PROD,
personal account, real friend, key rotation or receiver takeover is authorized.
QA credentials and App Check token come from the local secret store through
stdin; discover their references through local memory, never put them here.

## Prepare

On BLACKSTAR use A `acceptance-r161` (7707, frontend 15007, CDP 9407) and B
`curated` (7697, frontend 14997, CDP 9397). Both use the `qa` account target,
`CODETERM_DEV_WATCH=0`, separate retained data roots and the same canonical
named-dev build. The build owner stops other holders before compiling once;
B adopts A's build through `dev-prebuilt`. Never build in this lane.

Privately confirm the Buyer and free/QA-staff Peer aliases, distinct signed-in
identities and their reciprocal QA friendship. Preserve existing state. If
sign-in is needed, follow qa-peers' supported Browser App Check debug-token
flow. Complete the real General permission interview if no grant exists.
The existing scope must permit a synthetic public verification code and
trusted linked replies. Leave unrelated grants and limits intact. A permitted
controlled folder must already exist. Changing machine access restarts the
concierge with fresh provider context; grant edits below are the authorized
three-access acceptance matrix, not a request to widen a real friend's access.

Create a private config outside tracked source, replacing the example values:

```json
{
  "candidate": "FULL_40_HEX_CANDIDATE_SHA",
  "friendship": "EXISTING_64_HEX_QA_FRIENDSHIP_ID",
  "qa_accounts_confirmed": true,
  "peer": { "instance": "curated", "port": 7697 }
}
```

`qa_accounts_confirmed` records the operator's private alias comparison; it is
not a substitute for the runner's independent backend, signed-in identity,
distinct-account and reciprocal-friendship checks. Both peers must report
`domios-signin-qa`, healthy receiving listeners and the exact candidate.
Missing prerequisites fail before a question is sent. The config and run
artifacts contain no credentials. Account emails and UIDs never enter results.

For an existing grant limited to a public fixture, add `question` and
`expected_answer` to the private config. The question must contain `{nonce}`,
replaced with a fresh per-run/access marker, and request only the granted public
fact. Read the controlled fixture independently to choose its exact expected
answer; do not widen the stored scope to fit the default code question. Only B
needs an idle bound concierge before grant edits: A's owner conversation can
receive its linked answer without a live concierge.

## Run and inspect

Use one Team QA utility with timeout 60m. Select A before sourcing the DEV lib;
the runner resolves B independently through the same lib. `CORE_JOURNEY_BUILD_CHECKOUT`
lets the lane's journey drive the build owner's unchanged canonical candidate.

```bash
export CODETERM_INSTANCE=acceptance-r161 CODETERM_API_PORT=7707
export CORE_JOURNEY_BUILD_CHECKOUT=/c/Developer/codeterm/codeterm
bash evals/core-journeys/run.sh all --journeys J5 --neighbors-config PRIVATE_CONFIG \
  --out OWNED_FRESH_OUT
node evals/core-journeys/lib/evidence-contract.mjs OWNED_FRESH_OUT
```

J5 uses the stored concierge provider/model and records actual session/model
identity. It does not run the J1–J4 complete-provider matrix, set global agent
full access, provision Teams or restart a DEV. A combined selection still runs
the existing J1–J4 model policy and requires their explicit `--model` options.
The API oracle requires the expected answer from B's exact bound concierge
author, exact `in_reply_to`, the same answer ID on A, delivered receipts on both
ends, `handling_state=answered` and the same B session idle. A local chat answer,
CLI exit 0, a question's delivered receipt or a refusal cannot pass it.

For each access level, use the **Domios Browser plugin** to open B's friend
conversation and friend page. Capture the current working indicator if seen;
after the linked answer is delivered, require the conversation's Stop/working
indicator to disappear and the friend page to report connected/ready. Save a
redacted Browser snapshot and screenshot with the candidate, access, question
and answer IDs in the run ledger. A missed working transition is not proof it
occurred; API handling evidence and the final visible idle state remain separate.
The current friend page itself labels readiness as connected/ready, not the
literal word idle. The runner records its UI leg as **NOT RUN**: its API PASS
does not establish this curated Browser check. J5-only runs do not use native
CDP or a native screenshot, so this same API journey can run on macOS. Release
clearance requires the independently reviewed Browser evidence too.

The runner restores only B's original folder/access at a quiescent boundary
and reads it back. On active or uncertain handling it retains the grant and
reports cleanup failure so the owner can reconcile the exact pending IDs.
Do not stop/reopen the concierge or replay a payload to simplify cleanup.
Every paid run needs a new output directory; an existing J5 state refuses
another setup. A transport uncertainty is retained, never resubmitted.

## Fail-first and platform ledger

Before merging/adopting the concierge fix, retain a Windows run against the
qualified current pre-fix 1.20.2 head. It must fail on `read_only` because no
trusted linked answer is delivered. A failed qualification is a prerequisite
failure, not an incident reproduction. Keep raw exits, run directories,
candidate SHAs, UTC, native/frontend proof and the failed handling/CLI reason.
Only then may the build owner advance both peers to the real-merged fixed
candidate and run a fresh full matrix. Never overwrite the first failure.

On macOS, the assigned air/eight owner runs the same journey with its qualified
disposable named QA pair and explicit private ports. Run the harness on the
machine hosting that pair, substitute its local canonical checkout and B's
name/port in the config, and preserve macOS native/frontend qualification.
Do not use account-mesh/owner friends as a substitute for the two QA accounts.
Windows results do not qualify macOS; absent native results are NOT RUN.

| Platform/candidate | read_only | folder | full | Browser idle evidence |
| --- | --- | --- | --- | --- |
| Windows pre-fix | required FAIL reproduction | not required | not required | retain failure |
| Windows fixed release candidate | required PASS | required PASS | required PASS | required |
| macOS fixed release candidate (air/eight) | required PASS | required PASS | required PASS | required |

Keep actual run outcomes in plan/release evidence, not this procedure. The
release skill refuses clearance while a selected platform/access/UI row is
missing or failed. The API evidence validator refuses a purported passing
matrix with any missing access, linked answer, far-end receipt or idle binding.

One small synthetic attachment is optional while `account neighbors send
--attach` is unavailable. When it lands, use its documented own-pad admission
once, record the attachment ID/hash and independently verify the saved file on
B. A text answer never substitutes for file receipt. Do not implement a private
upload path in this journey to bypass the CLI's current capability.
