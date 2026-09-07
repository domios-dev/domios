#!/usr/bin/env node

const [operation, ...args] = process.argv.slice(2);
const input = await new Promise((resolve) => {
  let value = "";
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", (chunk) => { value += chunk; });
  process.stdin.on("end", () => resolve(value));
});
const data = input ? JSON.parse(input) : null;
const plan = data?.plan ?? data;

const write = (value) => process.stdout.write(value == null ? "" : String(value));
const failUnless = (condition) => {
  if (!condition) process.exit(1);
};
const containsExactString = (value, token) => {
  if (value === token) return true;
  if (Array.isArray(value)) return value.some((item) => containsExactString(item, token));
  if (value && typeof value === "object") {
    return Object.values(value).some((item) => containsExactString(item, token));
  }
  return false;
};
const tasks = () => plan?.tasks ?? [];

switch (operation) {
  case "workspace-owned":
    failUnless(data?.existing !== true && data?.cwd === args[0]
      && typeof data?.id === "string" && data.id.length > 0);
    write(data.id);
    break;
  case "tab-id":
    write(data?.tab_id ?? data?.id);
    break;
  case "plan-id":
    write(data?.plan?.id ?? data?.id);
    break;
  case "team-id":
    write(data?.team_id);
    break;
  case "team-minted":
    failUnless(data?.team_minted === true);
    break;
  case "tab-nonce": {
    const tab = Array.isArray(data) ? data[0] : (data?.tabs?.[0] ?? data);
    write(tab?.session_binding_nonce);
    break;
  }
  case "session-body":
    write(JSON.stringify({ session_id: args[0], binding_nonce: args[1], source: "plan-authorized-reporting" }));
    break;
  case "bind-ok":
    failUnless(data?.ok === true && data?.session_id === args[0]);
    break;
  case "token-absent":
    failUnless(!containsExactString(data, args[0]));
    break;
  case "report-token-absent":
    failUnless(!data.some((report) => report.message === args[0]));
    break;
  case "event-type":
    failUnless(data?.event_type === "agent-report");
    break;
  case "plan-snapshot":
    write(JSON.stringify({
      strides: plan?.strides ?? [],
      tasks: tasks().map(({ id, status, assigned_tab_id, milestones }) => ({ id, status, assigned_tab_id, milestones })),
    }));
    break;
  case "task-id":
    write(data?.task?.id ?? data?.id);
    break;
  case "task-snapshot": {
    const task = tasks().find((item) => item.id === args[0]);
    if (!task) process.exit(1);
    write(JSON.stringify({ status: task.status, assigned_tab_id: task.assigned_tab_id, milestones: task.milestones }));
    break;
  }
  case "accepted-response":
    failUnless(data?.accepted === true
      && data?.plan_report?.persisted === true
      && data?.plan_report?.plan_id === args[0]
      && data?.plan_report?.task_id == null
      && data?.plan_report?.task_transition === false);
    break;
  case "plan-evidence":
    failUnless((plan?.strides ?? []).some((stride) => stride.kind === "plan_evidence_report"
      && stride.text === args[0] && stride.from?.tab_id != null));
    break;
  case "owner-report":
    failUnless(data.some((report) => report.message === args[0]
      && report.task_id == null && report.status == null));
    break;
  default:
    throw new Error(`unknown JSON operation: ${operation}`);
}
