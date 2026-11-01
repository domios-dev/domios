import { recognize } from "./recognize";
import { renderTemplate, parseSend, planLink, isHelp, isSend, navigationTarget, describeAction } from "./templates";

let passed = 0;
let failed = 0;
function assert(cond: unknown, name: string) {
  if (cond) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}`);
  }
}

function inv(cmd: string) {
  const i = recognize(cmd);
  if (!i) throw new Error(`recognize failed: ${cmd}`);
  return i;
}

const PLAN = "pl_" + "a".repeat(32);
const TASK = "task_" + "b".repeat(32);

// --- send detection + parse ---
{
  assert(isSend(inv(`codeterm send --tab bda "hi"`)), "isSend true for send");
  assert(!isSend(inv("codeterm send --help")), "send help is not an outgoing message");
  assert(isHelp(inv("codeterm send --help")), "long help flag detected");
  assert(isHelp(inv("codeterm send -h")), "short help flag detected");
  assert(!isSend(inv(`codeterm mem search "x"`)), "isSend false for mem");

  const m = parseSend(inv(`codeterm send --tab 915fae8e "hello"`), undefined, undefined);
  assert(m.recipient === "tab 915fae8e", "recipient falls back to tab <ref>");
  assert(m.body === "hello", "body is positional");
  assert(m.formattedAvailable, "typed message body can be formatted");
  assert(m.mesh === undefined, "no mesh for local send");

  const named = parseSend(inv(`codeterm send --tab 915fae8e "hi"`), undefined, (r) => (r === "915fae8e" ? "Puccini" : undefined));
  assert(named.recipient === "Puccini", "recipient resolves to tab name");
  assert(named.recipientId === "915fae8e" && named.recipientTitleResolved, "recipient keeps its stable tab ref after title resolution");
  for (const wrapper of [
    `pwsh.exe -Command "codeterm send --tab 915fae8e --from-pad msg.md"`,
    `bash -lc 'codeterm send --tab 915fae8e --from-pad msg.md'`,
    `cmd.exe /c "codeterm send --tab 915fae8e --from-pad msg.md"`,
  ]) {
    const shape = recognize(wrapper);
    assert(shape != null && isSend(shape), `recognizes send inside shell wrapper: ${wrapper.split(" ")[0]}`);
  }

  const mesh = parseSend(inv(`codeterm send --tab bda --mesh mini "yo"`), undefined, undefined);
  assert(mesh.mesh === "mini", "mesh peer parsed");

  const ok = parseSend(inv(`codeterm send --tab bda "x"`), "OK (sent)", undefined);
  assert(ok.status?.ok === true && ok.status.label === "sent", "sent marker does not imply delivery");
  const deliveredText = parseSend(inv(`codeterm send --tab bda "x"`), "delivered", undefined);
  assert(deliveredText.status?.ok === true && deliveredText.status.label === "delivered", "distinct delivery marker preserves delivered state");
  const pending = parseSend(inv(`codeterm send --tab bda "x"`), "OK (pending)", undefined);
  assert(pending.status?.pending === true && pending.status.label === "pending", "accepted send stays visibly pending");
  assert(!pending.status?.ok, "pending send is never marked delivered");
  const genericSuccess = parseSend(inv(`codeterm send --tab bda "x"`), JSON.stringify({ result: { ok: true } }));
  assert(genericSuccess.status?.label === "pending" && !genericSuccess.status.ok, "generic command success without delivery evidence stays pending");
  const resolved = parseSend(inv(`codeterm send --tab bda "$VAR"`), 'OK (sent)\nCODETERM_RESOLVED_TEXT="expanded ***REDACTED*** text"', undefined);
  assert(resolved.body === "expanded ***REDACTED*** text" && resolved.bodySource === "resolved", "resolved output overrides raw source");
  const structured = parseSend(inv(`codeterm send --tab bda --from-pad message.md`), JSON.stringify({ result: { delivery: "pending", sent_message: { text: "Recorded markdown **body**", truncated: false } } }));
  assert(structured.body === "Recorded markdown **body**" && structured.formattedAvailable, "structured send evidence supplies staged body");
  assert(structured.status?.pending === true && !structured.status.ok, "structured pending status never appears sent");
  for (const [delivery, label] of [["submit_unconfirmed", "submit_unconfirmed"], ["interrupted", "interrupted"], ["unresolved", "unresolved"]] as const) {
    const state = parseSend(inv(`codeterm send --tab bda "x"`), JSON.stringify({ result: { delivery } }));
    assert(state.status?.label === label && !state.status.ok, `${delivery} transport outcome stays distinct`);
  }
  const structuredFailure = parseSend(inv(`codeterm send --tab bda "x"`), JSON.stringify({ result: { delivery: "error", error: "tab unavailable" } }));
  assert(structuredFailure.status?.label === "failed" && structuredFailure.status.detail === "tab unavailable", "structured transport failure keeps its error detail");
  const contradictoryStructured = parseSend(inv(`codeterm send --tab bda "x"`), JSON.stringify({ ok: false, error: "transport failed", result: { delivery: "sent" } }));
  assert(contradictoryStructured.status?.label === "failed", "structured failure evidence takes precedence over sent delivery data");
  for (const [delivery, label] of [["sent", "sent"], ["delivered", "delivered"]] as const) {
    const state = parseSend(inv(`codeterm send --tab bda "x"`), JSON.stringify({ result: { ok: true, delivery } }));
    assert(state.status?.label === label && state.status.ok, `explicit ${delivery} evidence takes precedence over generic result success`);
  }
  const deliveredArray = parseSend(inv(`codeterm send --tab bda "x"`), JSON.stringify({ result: { ok: true, delivered: ["message_1"] } }));
  assert(deliveredArray.status?.label === "delivered", "delivered result array is distinct delivery evidence");
  const sentArray = parseSend(inv(`codeterm send --tab bda "x"`), JSON.stringify({ result: { ok: true, sent: ["message_1"] } }));
  assert(sentArray.status?.label === "sent", "sent result array does not imply delivery");
  const contradictoryDelivery = parseSend(inv(`codeterm send --tab bda "x"`), JSON.stringify({ result: { ok: true, delivery: "delivered", error: "transport failed" } }));
  assert(contradictoryDelivery.status?.label === "failed" && contradictoryDelivery.status.detail === "transport failed", "structured error overrides contradictory delivered success");
  const pendingWithError = parseSend(inv(`codeterm send --tab bda "x"`), JSON.stringify({ result: { delivery: "pending", error: "transport rejected" } }));
  assert(pendingWithError.status?.label === "failed" && pendingWithError.status.detail === "transport rejected", "structured errors remain visible beside a pending delivery marker");
  const contradictoryText = parseSend(inv(`codeterm send --tab bda "x"`), "Exit code: 2\nOK (sent)");
  assert(contradictoryText.status?.label === "failed", "explicit nonzero exit evidence takes precedence over a sent marker");
  const tooLong = parseSend(inv(`codeterm send --tab bda --from-pad message.md`), JSON.stringify({ result: { delivery: "sent", sent_message: { text: "x".repeat(16_385), truncated: true } } }));
  assert(tooLong.body.length === 16_384 && tooLong.truncated === true, "structured body is bounded and marks truncation");
  const typed = parseSend(inv(`codeterm send --tab bda "$VAR"`), "OK (sent)", undefined);
  assert(typed.body === "$VAR" && typed.bodySource === "typed", "missing resolved output is explicitly source as typed");
  const fromFile = parseSend(inv(`codeterm send --tab bda --from-file update.md`), "OK (sent)", undefined);
  assert(fromFile.body.length > 0 && fromFile.body.includes("update.md") && fromFile.recipient === "tab bda" && fromFile.status?.ok === true, "from-file send has a delivered body descriptor");
  assert(!fromFile.formattedAvailable, "unresolved file body falls back to raw view");
  const fileAlias = parseSend(inv(`codeterm send --tab bda --file update.md`), "OK (sent)", undefined);
  assert(fileAlias.body === fromFile.body && fileAlias.status?.ok === true, "file alias uses the canonical file descriptor");
  const stdin = parseSend(inv(`codeterm send --tab bda --stdin`), "OK (sent)", undefined);
  assert(stdin.body.length > 0 && stdin.recipient === "tab bda" && stdin.status?.ok === true, "stdin send has a delivered body descriptor");
  const fromPad = parseSend(inv(`codeterm send --tab bda --from-pad message.md`), "OK (sent)", undefined);
  assert(fromPad.body === "AgentPad payload: message.md" && fromPad.bodySource === "pad", "AgentPad send is identified without exposing its runtime path");
  assert(!fromPad.formattedAvailable, "unresolved AgentPad body falls back to raw view");
  const padRedirect = parseSend(inv(`codeterm send --from-pad dumas-final-report.txt --tab 166df18d --mesh BLACKSTAR 2>&1`), "OK (sent)", undefined);
  assert(padRedirect.body === "AgentPad payload: dumas-final-report.txt" && padRedirect.bodySource === "pad" && !padRedirect.body.includes("2>&1"), "AgentPad send never renders a trailing shell redirect positional as its body");
  const regression = parseSend(inv(`codeterm send --tab bda`), "OK (sent)", undefined);
  assert(regression.body.length > 0 && regression.recipient === "tab bda" && regression.status?.ok === true, "delivered send without body sources has a non-empty descriptor");
  const fail = parseSend(inv(`codeterm send --tab bda "x"`), "error: Tab not found", undefined);
  assert(fail.status?.ok === false && fail.status.label === "failed", "failed status");
  assert(fail.status?.detail === "Tab not found", "plain-text failure keeps its visible reason");

  const none = parseSend(inv(`codeterm send "x"`), undefined, undefined);
  assert(none.recipient === "current tab", "current tab when no --tab");
}

// --- plan templates + link extraction ---
{
  // A fixture using --plan/--title/--kind renders a card no user can ever produce, so it proves nothing about the renderer.
  const addCard = renderTemplate(inv(`codeterm plan task add ${PLAN} --name "Fix login"`));
  assert(addCard.title === "Task created", "task input is not treated as authoritative result metadata");
  assert(addCard.link?.planId === PLAN, "plan task add carries the positional plan id");

  const addTitleFlag = renderTemplate(inv(`codeterm plan task add ${PLAN} --title "Fix login"`));
  assert(addTitleFlag.title === "Task created", "a --title that clap rejects never names the created task");

  const addWithTextId = renderTemplate(inv(`codeterm plan task add ${PLAN}`), `created ${TASK}`);
  assert(addWithTextId.link?.taskId === undefined, "unstructured text cannot supply a task identity");

  const taskResult = JSON.stringify({ ok: true, result: { task_id: TASK, key: "COD-2212", section_ids: [] } });
  const created = renderTemplate(inv(`codeterm plan task add ${PLAN} --name "Fix login"`), taskResult);
  assert(created.title === "Task created: Fix login", "create retains the accepted --name after the service returns its mutation id");
  assert(created.link?.taskId === TASK, "create opens the returned exact task id");
  assert(navigationTarget(inv(`codeterm plan task add ${PLAN}`), taskResult)?.id === TASK, "create navigation comes from the structured task record");
  const humanCreated = renderTemplate(inv(`codeterm plan task add ${PLAN} --name "Human output task"`), `task_id ${TASK}`);
  assert(humanCreated.title === "Task created: Human output task" && humanCreated.link?.taskId === TASK, "create accepts the CLI's exact labeled mutation id output");
  const failedCreate = renderTemplate(inv(`codeterm plan task add ${PLAN} --name "Rejected task"`), "Error: required task fields are missing");
  assert(failedCreate.title === "Task creation failed" && failedCreate.link?.taskId === undefined, "failed create keeps the error and does not claim the requested task was created");
  assert(navigationTarget(inv(`codeterm plan task add ${PLAN}`), `created ${TASK}`) === null, "task ids in arbitrary prose are not accepted");

  const upd = renderTemplate(inv(`codeterm plan task update ${PLAN} ${TASK} --status in_review`));
  assert(upd.title === "Task updated", "unresolved task update does not claim a new authoritative status");
  assert(upd.link?.taskId === TASK && upd.link?.planId === PLAN, "update card carries positional plan+task ids");

  const updatedResult = JSON.stringify({ ok: true, result: { ok: true } });
  const updated = renderTemplate(inv(`codeterm plan task update ${PLAN} ${TASK} --name "Renamed task" --status in_review`), updatedResult);
  assert(updated.title === "Task updated: Renamed task", "successful update retains the accepted --name when the service returns ok");
  assert(updated.link?.taskId === TASK, "update retains the exact task opener");
  const humanUpdated = renderTemplate(inv(`codeterm plan task update ${PLAN} ${TASK} --name "Human update"`), "OK");
  assert(humanUpdated.title === "Task updated: Human update" && humanUpdated.link?.taskId === TASK, "update accepts the CLI's exact OK mutation output and command task id");
  const failedUpdate = renderTemplate(inv(`codeterm plan task update ${PLAN} ${TASK} --name "Rejected rename"`), "Exit code: 2\nupdate rejected");
  assert(failedUpdate.title === "Task update failed" && failedUpdate.link?.taskId === TASK, "failed update keeps its exact command target without applying the requested title");
  const mismatch = JSON.stringify({ result: { task_id: "task_different", key: "COD-9" } });
  assert(navigationTarget(inv(`codeterm plan task update ${PLAN} ${TASK}`), mismatch) === null, "conflicting result identity cannot navigate to a different task");
  const conflictingRecords = JSON.stringify({ result: { task_id: TASK, task: { id: "task_different" } } });
  assert(navigationTarget(inv(`codeterm plan task add ${PLAN}`), conflictingRecords) === null, "conflicting returned task identities cannot open either record");
  const taskDescriptor = describeAction(inv(`codeterm plan task update ${PLAN} ${TASK} --name "Renamed task" --model gpt --status in_review`), updatedResult);
  assert(taskDescriptor.object === "Renamed task", "task row summary retains the successful mutation name");
  assert(taskDescriptor.chips.includes("in_review"), "successful command status remains visible as secondary detail");
  assert(taskDescriptor.chips.includes("gpt"), "task model metadata remains visible");
  const failedDescriptor = describeAction(inv(`codeterm plan task add ${PLAN} --name "Rejected task"`), "Error: invalid fields");
  assert(failedDescriptor.object !== "Rejected task", "failed task creation does not use its requested name as if created");
  const pendingDescriptor = describeAction(inv(`codeterm plan task add ${PLAN} --name "Pending task"`), undefined, undefined, undefined, undefined, "pending");
  assert(pendingDescriptor.object !== "Pending task", "unconfirmed task creation does not promote the requested name as a completed task");
  const requestedDescriptor = describeAction(inv(`codeterm plan task update ${PLAN} ${TASK} --name "Requested update"`));
  assert(requestedDescriptor.object === "Requested update", "task rows retain an explicit requested name when no failure is reported");
  assert(describeAction(inv(`codeterm plan task close ${PLAN} ${TASK}`)).object === TASK, "task close keeps its existing exact reference summary");

  const rec = renderTemplate(inv(`codeterm plan record ${PLAN} --step "Chose Postgres\nmore"`));
  assert(rec.title === "Progress recorded", "plan record title without --status");
  assert(rec.detail === "Chose Postgres", "plan record detail is the --step note, first line only");
  assert(rec.detail !== PLAN, "plan record never renders the positional plan id as the stride note");
  assert(rec.link?.planId === PLAN, "plan record link from the positional plan id");

  const recStatus = renderTemplate(inv(`codeterm plan record ${PLAN} --task ${TASK} --status in_review --step "wired up"`));
  assert(recStatus.title === "Progress recorded → in_review", "plan record surfaces the --status it actually writes");
  assert(recStatus.link?.taskId === TASK, "plan record carries --task through to the link");

  const state = renderTemplate(inv(`codeterm plan state ${PLAN}`));
  assert(state.title === "Plan state", "plan state compact");
  assert(state.link?.planId === PLAN, "plan state link from positional");

  assert(planLink(inv(`codeterm plan state`)) === undefined, "no link without any plan id");

  // describeAction reads the same invocation; positional 0 is the plan id there too.
  const desc = describeAction(inv(`codeterm plan record ${PLAN} --step "Chose Postgres"`));
  assert(desc.detail === "Chose Postgres", "descriptor detail is the --step note");
  assert(desc.detail !== PLAN, "descriptor never falls back to the positional plan id");

  const descNoStep = describeAction(inv(`codeterm plan record ${PLAN}`));
  assert(descNoStep.detail === undefined, "no note flag means no detail, not the plan id");
}

// --- agent report card body fields ---
{
  const BODY = [
    "STATUS: in_review",
    "CHANGES: cli/src/cmd/report_body.rs",
    "TESTS RUN: deferred to canonical QA",
    "TESTS COVERING CHANGES: build test-one cli report_body 4 passed",
    "GATES REQUESTED: build test-one cli report_echo",
    "FOLLOWUP: orchestrator closes the task",
  ].join("\n");
  const EXPECTED = JSON.stringify([
    { label: "status", value: "in_review" },
    { label: "changes", value: "cli/src/cmd/report_body.rs" },
    { label: "tests", value: "deferred to canonical QA" },
    { label: "tests covering changes", value: "build test-one cli report_body 4 passed" },
    { label: "gates requested", value: "build test-one cli report_echo" },
    { label: "followup", value: "orchestrator closes the task" },
    { label: "sha", value: "a56ee3e3b" },
  ]);
  const echo = `report_r1 recorded\nCODETERM_RESOLVED_TEXT=${JSON.stringify(BODY)}`;

  const inlineToday = describeAction(inv(`codeterm agent report --status in_review --sha a56ee3e3b "${BODY}"`), "report_r1 recorded");
  assert(JSON.stringify(inlineToday.fields) === EXPECTED, "regression control: an inline report with no resolved echo renders exactly today's argv-extracted fields");

  const inlineEchoed = describeAction(inv(`codeterm agent report --status in_review --sha a56ee3e3b "${BODY}"`), echo);
  assert(JSON.stringify(inlineEchoed.fields) === EXPECTED, "an inline report renders identically once the CLI also echoes the resolved body");

  const fromPad = describeAction(inv(`codeterm agent report --status in_review --sha a56ee3e3b --from-pad report.md`), echo);
  assert(JSON.stringify(fromPad.fields) === EXPECTED, "a --from-pad report renders every body field from the resolved echo");

  const padWithoutEcho = describeAction(inv(`codeterm agent report --status in_review --sha a56ee3e3b --from-pad report.md`), "report_r1 recorded");
  assert(JSON.stringify(padWithoutEcho.fields) === JSON.stringify([{ label: "status", value: "in_review" }, { label: "sha", value: "a56ee3e3b" }]), "without a resolved echo a pad report still collapses to flags only — the defect this fixes");

  const textFlag = describeAction(inv(`codeterm agent report --status in_review --sha a56ee3e3b --text "${BODY}"`), "report_r1 recorded");
  assert(JSON.stringify(textFlag.fields) === EXPECTED, "--text extraction is unchanged when no echo is present");

  const ESCAPED = BODY.replace(/\n/g, "\\n");
  const escapedToday = describeAction(inv(`codeterm agent report --status in_review --sha a56ee3e3b "${ESCAPED}"`), "report_r1 recorded");
  const escapedEchoed = describeAction(inv(`codeterm agent report --status in_review --sha a56ee3e3b "${ESCAPED}"`), `report_r1 recorded\nCODETERM_RESOLVED_TEXT=${JSON.stringify(ESCAPED)}`);
  assert(JSON.stringify(escapedToday.fields) === EXPECTED && JSON.stringify(escapedEchoed.fields) === EXPECTED, "an inline body carrying literal \\n escapes renders the same body fields with and without the echo");

  const redacted = describeAction(inv(`codeterm agent report --status in_review --from-pad report.md`), `ok\nCODETERM_RESOLVED_TEXT=${JSON.stringify("STATUS: in_review\nCHANGES: API_TOKEN=***REDACTED***")}`);
  assert(redacted.fields?.some((f) => f.label === "changes" && f.value === "API_TOKEN=***REDACTED***"), "the card renders whatever the CLI redacted, never a second unredacted source");
}

// --- lead and object print a command once ---
{
  const commands = [
    "codeterm plan doctor",
    "codeterm plan decision list",
    "codeterm task show",
    "codeterm spec section update --help",
    "codeterm plan task update --help",
    "codeterm send --help",
    "codeterm tab close tab_8f1e2d",
    `codeterm mem search "query"`,
  ];
  for (const command of commands) {
    const invocation = inv(command);
    const { lead, object } = describeAction(invocation);
    const label = object ? `${lead} ${object}` : lead;
    const path = invocation.path.join(" ");
    assert(object === undefined || object.trim().toLowerCase() !== lead.trim().toLowerCase(), `${command}: object never repeats the lead`);
    assert(label.split(path).length <= 2, `${command}: the command path appears at most once in its label`);
  }
  const doctor = describeAction(inv("codeterm plan doctor"));
  assert(doctor.lead === "plan doctor" && doctor.object === undefined, "a path without its own template is its label, printed once");
  const help = describeAction(inv("codeterm spec section update --help"));
  assert(help.lead === "spec section update" && help.object === "help", "help leads with the command it documents");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
