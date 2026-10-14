import { kind, textContent } from "../../restart-endurance/oracle.mjs";

export const REQUIRED_PROVIDERS = ["claude", "codex", "opencode"];
const TERMINAL_STAGES = new Set(["spawn_failed", "launch_deadline_elapsed", "launch_binding_unconfirmed", "awaiting_owner_prompt"]);
const READY_STAGES = new Set(["agent_ready", "pty_reattached"]);
const FAILED_RUNTIME = new Set(["degraded", "failed"]);

export function installed(descriptor) {
  return descriptor?.availability === "available" &&
    (descriptor.cli_status == null || descriptor.cli_status.installed === true);
}

export function isChatBackend(record) {
  return record?.backing?.kind === "chat_backend";
}

export function runsWithoutPty(descriptor) {
  return descriptor?.transport === "chat_backend" || descriptor?.agent_runtime != null;
}

export function selectProviders(descriptors, { required = REQUIRED_PROVIDERS, only = null } = {}) {
  const list = Array.isArray(descriptors) ? descriptors : [];
  const wanted = only ?? list.filter(installed).map((descriptor) => descriptor.id);
  const byId = new Map(list.map((descriptor) => [descriptor.id, descriptor]));
  const run = [], missing = [];
  for (const id of new Set([...(only ? [] : required), ...wanted])) {
    const descriptor = byId.get(id);
    if (descriptor && installed(descriptor)) run.push({ id, runtime: runsWithoutPty(descriptor) });
    else missing.push({ id, reason: descriptor ? `availability=${descriptor.availability ?? "unknown"} installed=${descriptor.cli_status?.installed ?? "unknown"}` : "provider not registered" });
  }
  return { run, missing };
}

export function runtimeFailure(record) {
  const runtime = record?.agent_runtime_state;
  if (runtime && FAILED_RUNTIME.has(runtime.state)) return `runtime ${runtime.state}: ${runtime.reason ?? "no reason reported"}`;
  return null;
}

export function readiness(record, { requireSession = false } = {}) {
  if (!record) return { fail: "tab absent from the tab projection" };
  if (TERMINAL_STAGES.has(record.boot_stage)) {
    return { fail: `${record.boot_stage}: ${record.boot_failure_reason ?? JSON.stringify(record.pending_prompt ?? null)}` };
  }
  const runtime = runtimeFailure(record);
  if (runtime) return { fail: runtime };
  if (record.pending_prompt) return { fail: `owner prompt pending: ${JSON.stringify(record.pending_prompt).slice(0, 300)}` };
  if (record.status === "exited" || record.status === "error") return { fail: `tab status ${record.status}` };
  const staged = record.agent_type && READY_STAGES.has(record.boot_stage);
  if (staged && (record.agent_session_id || (!requireSession && record.session_binding_nonce))) return { done: record };
  return { pending: `ready: boot_stage=${record.boot_stage ?? "none"} session=${record.agent_session_id ?? "none"}` };
}

export function normalizeAnswer(text) {
  const unwrapped = String(text ?? "").trim().replace(/\.$/, "").replace(/^[`'"*\s]+|[`'"*\s]+$/g, "");
  return unwrapped.replace(/\.$/, "");
}

export function newAssistantText(messages, baselineIds) {
  return (messages ?? []).filter((message) => kind(message) === "assistant" && !baselineIds.has(message.id))
    .map((message) => textContent(message.content).trim()).filter(Boolean);
}

export function replyVerdict({ messages, baselineIds, marker, record, boundSession }) {
  const texts = newAssistantText(messages, baselineIds);
  const failure = runtimeFailure(record);
  if (failure) return { fail: failure };
  if (boundSession && record?.agent_session_id && record.agent_session_id !== boundSession) {
    return { fail: `session changed during probe: ${boundSession} -> ${record.agent_session_id}` };
  }
  const exact = texts.some((text) => normalizeAnswer(text) === marker || normalizeAnswer(text.split("\n").at(-1)) === marker);
  if (exact && record?.status === "idle") return { done: { answer: marker, session: record.agent_session_id } };
  return { pending: exact ? "reply observed; waiting for idle" : `exact reply ${marker} (new assistant items: ${texts.length})` };
}

export function chatMessages(chat) {
  return Array.isArray(chat) ? chat : chat?.messages ?? chat?.data?.messages ?? [];
}

export function findTask(value) {
  if (!value || typeof value !== "object") return null;
  if (Array.isArray(value.execution_attempts) && typeof value.status === "string") return value;
  for (const key of ["task", "data"]) {
    const found = findTask(value[key]);
    if (found) return found;
  }
  return null;
}

export function currentAttempt(task) {
  const attempts = (task?.execution_attempts ?? []).filter((attempt) => !attempt.retired_at && !attempt.closed_at);
  return attempts.at(-1) ?? task?.execution_attempts?.at(-1) ?? null;
}

export function taskVerdict(task, want) {
  if (!task) return { pending: "task readable" };
  const attempt = currentAttempt(task);
  switch (want.kind) {
    case "acked":
      return attempt?.acknowledged_at ? { done: { task, attempt } } : { pending: `ACK (status=${task.status})` };
    case "reported": {
      const reportedAfter = attempt?.reported_at && attempt.reported_at > (want.after ?? 0);
      if (task.status === "in_review" && reportedAfter) return { done: { task, attempt } };
      if (["done", "cancelled"].includes(task.status)) return { fail: `task ${task.status} before review` };
      return { pending: `report (status=${task.status} reported_at=${attempt?.reported_at ?? "none"})` };
    }
    case "status":
      return task.status === want.status ? { done: { task, attempt } } : { pending: `status ${want.status} (now ${task.status})` };
    default:
      throw new Error(`unknown task verdict ${want.kind}`);
  }
}

export function attemptBindingVerdict(record, task) {
  const attempt = currentAttempt(task);
  if (!record) return { fail: "Worker tab absent after restart" };
  if (!attempt) return { fail: "task has no execution attempt" };
  const bound = record.task_binding?.attempt_id;
  if (!bound) return { fail: `restored Worker has no attempt_id in its task binding (task_binding=${JSON.stringify(record.task_binding ?? null)})` };
  if (bound !== attempt.id) return { fail: `restored Worker attempt_id ${bound} differs from current attempt ${attempt.id}` };
  if (attempt.executor_tab_id !== record.id) return { fail: `attempt executor ${attempt.executor_tab_id} is not the restored tab ${record.id}` };
  return { done: { attempt_id: bound } };
}

export function pairTabs(result) {
  const value = result?.data ?? result;
  const orchestrator = value?.orchestrator_tab_id, planner = value?.planner_tab_id;
  if (!orchestrator || !planner) return { fail: `pair result lacks tab ids: ${JSON.stringify(value).slice(0, 300)}` };
  return { done: { orchestrator, planner, plan_id: value.plan_id ?? null } };
}

export function stopVerdict(response, record) {
  const value = response?.data ?? response;
  if (value?.stopped !== true) return { fail: `stop refused: ${JSON.stringify(value).slice(0, 300)}` };
  if (record?.status === "idle") return { done: { session: record.agent_session_id } };
  return { pending: `idle after stop (status=${record?.status ?? "unknown"})` };
}
