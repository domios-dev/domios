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
  assert(m.mesh === undefined, "no mesh for local send");

  const named = parseSend(inv(`codeterm send --tab 915fae8e "hi"`), undefined, (r) => (r === "915fae8e" ? "Puccini" : undefined));
  assert(named.recipient === "Puccini", "recipient resolves to tab name");

  const mesh = parseSend(inv(`codeterm send --tab bda --mesh mini "yo"`), undefined, undefined);
  assert(mesh.mesh === "mini", "mesh peer parsed");

  const ok = parseSend(inv(`codeterm send --tab bda "x"`), "OK (sent)", undefined);
  assert(ok.status?.ok === true && ok.status.label === "delivered", "delivered status");
  const resolved = parseSend(inv(`codeterm send --tab bda "$VAR"`), 'OK (sent)\nCODETERM_RESOLVED_TEXT="expanded ***REDACTED*** text"', undefined);
  assert(resolved.body === "expanded ***REDACTED*** text" && resolved.bodySource === "resolved", "resolved output overrides raw source");
  const typed = parseSend(inv(`codeterm send --tab bda "$VAR"`), "OK (sent)", undefined);
  assert(typed.body === "$VAR" && typed.bodySource === "typed", "missing resolved output is explicitly source as typed");
  const fromFile = parseSend(inv(`codeterm send --tab bda --from-file update.md`), "OK (sent)", undefined);
  assert(fromFile.body.length > 0 && fromFile.body.includes("update.md") && fromFile.recipient === "tab bda" && fromFile.status?.ok === true, "from-file send has a delivered body descriptor");
  const fileAlias = parseSend(inv(`codeterm send --tab bda --file update.md`), "OK (sent)", undefined);
  assert(fileAlias.body === fromFile.body && fileAlias.status?.ok === true, "file alias uses the canonical file descriptor");
  const stdin = parseSend(inv(`codeterm send --tab bda --stdin`), "OK (sent)", undefined);
  assert(stdin.body.length > 0 && stdin.recipient === "tab bda" && stdin.status?.ok === true, "stdin send has a delivered body descriptor");
  const fromPad = parseSend(inv(`codeterm send --tab bda --from-pad message.md`), "OK (sent)", undefined);
  assert(fromPad.body === "AgentPad payload: message.md" && fromPad.bodySource === "pad", "AgentPad send is identified without exposing its runtime path");
  const padRedirect = parseSend(inv(`codeterm send --from-pad dumas-final-report.txt --tab 166df18d --mesh BLACKSTAR 2>&1`), "OK (sent)", undefined);
  assert(padRedirect.body === "AgentPad payload: dumas-final-report.txt" && padRedirect.bodySource === "pad" && !padRedirect.body.includes("2>&1"), "AgentPad send never renders a trailing shell redirect positional as its body");
  const regression = parseSend(inv(`codeterm send --tab bda`), "OK (sent)", undefined);
  assert(regression.body.length > 0 && regression.recipient === "tab bda" && regression.status?.ok === true, "delivered send without body sources has a non-empty descriptor");
  const fail = parseSend(inv(`codeterm send --tab bda "x"`), "error: Tab not found", undefined);
  assert(fail.status?.ok === false && fail.status.label === "failed", "failed status");

  const none = parseSend(inv(`codeterm send "x"`), undefined, undefined);
  assert(none.recipient === "current tab", "current tab when no --tab");
}

// --- plan templates + link extraction ---
{
  // Every fixture below is the real clap shape: `plan task add <plan_id> --name`,
  // `plan record <plan_id> --step`. A fixture using --plan/--title/--kind renders
  // a card no user can ever produce, so it proves nothing about the renderer.
  const addCard = renderTemplate(inv(`codeterm plan task add ${PLAN} --name "Fix login"`));
  assert(addCard.title === "Task created: Fix login", "plan task add reads --name, the flag clap requires");
  assert(addCard.link?.planId === PLAN, "plan task add carries the positional plan id");

  const addTitleFlag = renderTemplate(inv(`codeterm plan task add ${PLAN} --title "Fix login"`));
  assert(addTitleFlag.title === "Task created", "a --title that clap rejects never names the created task");

  const addWithTask = renderTemplate(inv(`codeterm plan task add ${PLAN}`), `created ${TASK}`);
  assert(addWithTask.link?.taskId === TASK, "task id recovered from result");

  const upd = renderTemplate(inv(`codeterm plan task update ${PLAN} ${TASK} --status in_review`));
  assert(upd.title === "Task status → in_review", "status-change card");
  assert(upd.link?.taskId === TASK && upd.link?.planId === PLAN, "update card carries positional plan+task ids");

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
    "GATES REQUESTED: build test-one cli report_echo",
    "FOLLOWUP: orchestrator closes the task",
  ].join("\n");
  const EXPECTED = JSON.stringify([
    { label: "status", value: "in_review" },
    { label: "changes", value: "cli/src/cmd/report_body.rs" },
    { label: "tests", value: "deferred to canonical QA" },
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
  assert(JSON.stringify(escapedToday.fields) === EXPECTED && JSON.stringify(escapedEchoed.fields) === EXPECTED, "an inline body carrying literal \\n escapes renders the same five fields with and without the echo");

  const redacted = describeAction(inv(`codeterm agent report --status in_review --from-pad report.md`), `ok\nCODETERM_RESOLVED_TEXT=${JSON.stringify("STATUS: in_review\nCHANGES: API_TOKEN=***REDACTED***")}`);
  assert(redacted.fields?.some((f) => f.label === "changes" && f.value === "API_TOKEN=***REDACTED***"), "the card renders whatever the CLI redacted, never a second unredacted source");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
