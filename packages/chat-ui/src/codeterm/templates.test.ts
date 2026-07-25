import { recognize } from "./recognize";
import { renderTemplate, parseSend, planLink, isHelp, isSend, navigationTarget } from "./templates";

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
  const fail = parseSend(inv(`codeterm send --tab bda "x"`), "error: pane not found", undefined);
  assert(fail.status?.ok === false && fail.status.label === "failed", "failed status");

  const none = parseSend(inv(`codeterm send "x"`), undefined, undefined);
  assert(none.recipient === "current tab", "current tab when no --tab");
}

// --- plan templates + link extraction ---
{
  const addCard = renderTemplate(inv(`codeterm plan task add --plan ${PLAN} --title "Fix login"`));
  assert(addCard.title === "Task created: Fix login", "plan task add title");
  assert(addCard.link?.planId === PLAN, "plan task add carries plan id");

  const addWithTask = renderTemplate(inv(`codeterm plan task add --plan ${PLAN}`), `created ${TASK}`);
  assert(addWithTask.link?.taskId === TASK, "task id recovered from result");

  const upd = renderTemplate(inv(`codeterm plan task update --plan ${PLAN} --task ${TASK} --status in_review`));
  assert(upd.title === "Task status → in_review", "status-change card");
  assert(upd.link?.taskId === TASK && upd.link?.planId === PLAN, "update card carries plan+task ids");

  const rec = renderTemplate(inv(`codeterm plan record --plan ${PLAN} --kind decision "Chose Postgres\nmore"`));
  assert(rec.title === "Plan updated: decision", "plan record title");
  assert(rec.detail === "Chose Postgres", "plan record first line only");
  assert(rec.link?.planId === PLAN, "plan record link");

  const state = renderTemplate(inv(`codeterm plan state ${PLAN}`));
  assert(state.title === "Plan state", "plan state compact");
  assert(state.link?.planId === PLAN, "plan state link from positional");

  assert(planLink(inv(`codeterm plan state`)) === undefined, "no link without any plan id");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
