import type { CodetermInvocation } from "./recognize";
import { decodeCliMessageText } from "./messageText";
import { describePluginAction, type PluginNameResolver } from "./pluginAction";

export interface CardStatus {
  ok: boolean;
  label: string;
  pending?: boolean;
  detail?: string;
}
export interface PlanLink {
  planId: string;
  taskId?: string;
}
export interface RenderedCard {
  icon: string;
  title: string;
  detail?: string;
  status?: CardStatus;
  link?: PlanLink;
}

export interface ActionDescriptor {
  lead: string;
  object?: string;
  chips: string[];
  detail?: string;
  bullets?: string[];
  fields?: Array<{ label: string; value: string }>;
}

/** Resolve a tab id/short-id to a human tab name. Returns undefined when unknown. */
export type TabNameResolver = (tabRef: string) => string | undefined;
/** Resolve a mesh peer id/name to its human instance name. */
export type PeerNameResolver = (peerRef: string) => string | undefined;

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f-]{27}$/i;

export function formatCommunicationEndpoint(label: string): string {
  const parts = label.split("@").map((part) => part.trim()).filter(Boolean);
  return parts.length > 1 ? `${parts.slice(0, -1).join(" @ ")} @ ${parts[parts.length - 1]}` : label.trim();
}

function friendlyPeerName(peerRef: string, resolve?: PeerNameResolver): string {
  return resolve?.(peerRef) ?? (UUID_RE.test(peerRef) ? "remote peer" : peerRef);
}

export type NavigationTarget =
  | { kind: "tab"; id: string }
  | { kind: "plan" | "task" | "section" | "spec" | "goal" | "memory" | "source"; id: string; planId?: string };

const TASK_RE = /task_[a-z0-9][a-z0-9_-]*/i;
const PLAN_RE = /pl_[a-z0-9][a-z0-9_-]*/i;
const SECTION_RE = /sec_[a-z0-9][a-z0-9_-]*/i;
const SPEC_RE = /(?:sp|spec)_[a-z0-9][a-z0-9_-]*/i;
const MEMORY_RE = /mem_[a-z0-9][a-z0-9_-]*/i;
const SOURCE_RE = /src_[a-z0-9][a-z0-9_-]*/i;
const TASK_ID_RE = /^task_[a-z0-9][a-z0-9_-]*$/i;
const PLAN_ID_RE = /^pl_[a-z0-9][a-z0-9_-]*$/i;

function strFlag(inv: CodetermInvocation, key: string): string | undefined {
  const v = inv.flags[key];
  return typeof v === "string" ? v : undefined;
}

function firstMatch(re: RegExp, ...texts: Array<string | undefined>): string | undefined {
  for (const t of texts) {
    if (!t) continue;
    const m = t.match(re);
    if (m) return m[0];
  }
  return undefined;
}

function firstRef(re: RegExp, inv: CodetermInvocation, result?: string): string | undefined {
  return firstMatch(re, ...Object.values(inv.flags).filter((value): value is string => typeof value === "string"), ...inv.positionals, result, inv.raw);
}

export interface TaskActivityMetadata {
  id: string;
  planId?: string;
  name?: string;
  key?: string;
  status?: string;
}

function record(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : undefined;
}

function nonBlank(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

interface ParsedTaskRecord {
  record?: TaskActivityMetadata;
  conflict?: boolean;
}

function taskRecord(result?: string): ParsedTaskRecord {
  if (!result || result.length > 65_536) return {};
  try {
    const envelope = record(JSON.parse(result));
    const payload = record(envelope?.result);
    if (!payload) return {};
    const task = record(payload.task);
    const taskId = nonBlank(task?.id) ?? nonBlank(task?.task_id);
    const mutationId = nonBlank(payload.task_id);
    if (taskId && mutationId && taskId !== mutationId) return { conflict: true };
    const id = taskId ?? mutationId;
    if (!id) return {};
    if (!TASK_ID_RE.test(id)) return { conflict: true };
    const taskPlanId = nonBlank(task?.plan_id);
    const mutationPlanId = nonBlank(payload.plan_id);
    if (taskPlanId && mutationPlanId && taskPlanId !== mutationPlanId) return { conflict: true };
    const planId = taskPlanId ?? mutationPlanId;
    if (planId && !PLAN_ID_RE.test(planId)) return { conflict: true };
    return {
      record: {
        id,
        planId,
        name: nonBlank(task?.name),
        key: nonBlank(task?.key) ?? nonBlank(task?.display_key) ?? nonBlank(payload.key) ?? nonBlank(payload.display_key),
        status: nonBlank(task?.status) ?? nonBlank(payload.status),
      },
    };
  } catch {
    const ids = [...result.matchAll(/^\s*task_id\s+(task_[a-z0-9][a-z0-9_-]*)\s*$/gim)].map((match) => match[1]);
    if (!ids.length) return {};
    if (new Set(ids).size > 1 || ids.some((id) => !TASK_ID_RE.test(id))) return { conflict: true };
    return { record: { id: ids[0] } };
  }
}

function taskMutationFailed(result?: string, activityStatus?: string): boolean {
  if (activityStatus === "error") return true;
  if (!result) return false;
  try {
    const envelope = record(JSON.parse(result));
    const payload = record(envelope?.result);
    if (envelope?.ok === false || envelope?.error != null || payload?.ok === false || payload?.error != null) return true;
  } catch {}
  return /^\s*(?:error\b|failed?\b|exit code:\s*[1-9]\d*|[^\r\n]*\bnot found\b)/im.test(result);
}

function taskMutationNameUnconfirmed(result?: string, activityStatus?: string): boolean {
  return taskMutationFailed(result, activityStatus) || (activityStatus !== undefined && activityStatus !== "success");
}

function taskMutationSucceeded(action: string | undefined, result: string | undefined, response: TaskActivityMetadata | undefined, activityStatus?: string): boolean {
  if ((activityStatus !== undefined && activityStatus !== "success") || taskMutationFailed(result, activityStatus)) return false;
  if (action === "add") return Boolean(response?.id);
  if (action !== "update") return false;
  if (response?.id) return true;
  if (!result || result.length > 65_536) return false;
  try {
    const envelope = record(JSON.parse(result));
    const payload = record(envelope?.result);
    return envelope?.ok !== false && envelope?.error == null && payload?.ok === true && payload.error == null;
  } catch {
    return result.split(/\r?\n/).map((line) => line.trim()).find(Boolean) === "OK";
  }
}

function commandPlanId(inv: CodetermInvocation): string | undefined {
  const flag = strFlag(inv, "plan");
  if (flag && PLAN_ID_RE.test(flag)) return flag;
  return inv.positionals.find((value) => PLAN_ID_RE.test(value));
}

function commandTaskId(inv: CodetermInvocation): string | undefined {
  const flag = strFlag(inv, "task");
  if (flag && TASK_ID_RE.test(flag)) return flag;
  return inv.positionals.find((value) => TASK_ID_RE.test(value));
}

function commandTaskDisplayRef(inv: CodetermInvocation): string | undefined {
  const flag = nonBlank(strFlag(inv, "task"));
  if (flag) return flag;
  const planId = commandPlanId(inv);
  return inv.positionals.find((value) => value !== planId && !value.startsWith("-"));
}

export function taskActivityMetadata(inv: CodetermInvocation, result?: string, activityStatus?: string): TaskActivityMetadata | undefined {
  if (inv.path[0] !== "plan" || inv.path[1] !== "task" || (inv.path[2] !== "add" && inv.path[2] !== "update")) return undefined;
  const parsedTask = taskRecord(result);
  if (parsedTask.conflict) return undefined;
  const responseTask = parsedTask.record;
  const inputTaskId = commandTaskId(inv);
  const inputPlanId = commandPlanId(inv);
  if (inputTaskId && responseTask && inputTaskId !== responseTask.id) return undefined;
  if (inputPlanId && responseTask?.planId && inputPlanId !== responseTask.planId) return undefined;
  const succeeded = taskMutationSucceeded(inv.path[2], result, responseTask, activityStatus);
  const id = inv.path[2] === "add"
    ? succeeded ? responseTask?.id : undefined
    : responseTask?.id ?? inputTaskId;
  if (!id) return undefined;
  return {
    id,
    planId: inputPlanId ?? responseTask?.planId,
    name: succeeded ? responseTask?.name ?? nonBlank(flagText(inv, "name")) : undefined,
    key: succeeded ? responseTask?.key : undefined,
    status: succeeded ? responseTask?.status ?? nonBlank(flagText(inv, "status")) : undefined,
  };
}

/** Describe the stable navigation target represented by a recognized invocation. */
export function navigationTarget(inv: CodetermInvocation, result?: string, activityStatus?: string): NavigationTarget | null {
  if (isSend(inv)) {
    if (typeof inv.flags.mesh === "string") return null;
    const candidate = inv.flags.tab ?? inv.flags.tab;
    const ref = typeof candidate === "string" ? candidate : undefined;
    return ref ? { kind: "tab", id: ref } : null;
  }
  if (inv.path[0] === "tab") {
    const candidate = inv.flags.tab ?? inv.flags.tab ?? inv.positionals[0];
    return typeof candidate === "string" ? { kind: "tab", id: candidate } : null;
  }
  if (inv.path[0] === "plan" && inv.path[1] === "task" && (inv.path[2] === "add" || inv.path[2] === "update")) {
    const task = taskActivityMetadata(inv, result, activityStatus);
    const planId = commandPlanId(inv) ?? task?.planId;
    return task && planId ? { kind: "task", id: task.id, planId } : null;
  }
  const planId = firstRef(PLAN_RE, inv, result);
  const taskId = firstRef(TASK_RE, inv, result);
  const sectionId = firstRef(SECTION_RE, inv, result);
  const specId = firstRef(SPEC_RE, inv, result);
  const memoryId = firstRef(MEMORY_RE, inv, result);
  const sourceFlag = inv.flags["source-path"] ?? inv.flags.sources;
  const sourceId = firstRef(SOURCE_RE, inv, result)
    ?? (typeof sourceFlag === "string" ? sourceFlag.split(",")[0]?.trim() : undefined);
  if (sectionId && planId) return { kind: "section", id: sectionId, planId };
  if (taskId && planId) return { kind: "task", id: taskId, planId };
  if (specId) return { kind: "spec", id: specId };
  if (inv.path[0] === "plan" && inv.path[1] === "goal" && planId) return { kind: "goal", id: planId, planId };
  if (memoryId) return { kind: "memory", id: memoryId };
  if (sourceId) return { kind: "source", id: sourceId };
  if (planId) return { kind: "plan", id: planId };
  return null;
}

function flagText(inv: CodetermInvocation, ...keys: string[]): string | undefined {
  for (const key of keys) {
    const value = inv.flags[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return undefined;
}

function firstMeaningful(inv: CodetermInvocation, ...skip: string[]): string | undefined {
  return inv.positionals.find((value) => value.trim() && !skip.includes(value));
}

function safeObject(value: string | undefined, fallback: string): string {
  return value && !/\b(?:pl|task|sec|sp|spec|mem|src)_[a-z0-9][a-z0-9_-]*/i.test(value) ? value : fallback;
}

function collectionLength(value: unknown): number | undefined {
  if (!value || typeof value !== "object") return undefined;
  const record = value as Record<string, unknown>;
  return Array.isArray(record.result) ? record.result.length : undefined;
}

function balancedJsonEnd(text: string, start: number): number | undefined {
  const expected: string[] = [];
  let quoted = false;
  let escaped = false;
  for (let index = start; index < text.length; index += 1) {
    const char = text[index];
    if (quoted) {
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === '"') quoted = false;
      continue;
    }
    if (char === '"') {
      quoted = true;
      continue;
    }
    if (char === "[") expected.push("]");
    else if (char === "{") expected.push("}");
    else if (char === "]" || char === "}") {
      if (expected.pop() !== char) return undefined;
      if (expected.length === 0) return index;
    }
  }
  return undefined;
}

function embeddedCollectionLength(result: string): number | undefined {
  const text = result.replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "").trim();
  const starts = new Set<number>();
  const first = text.search(/[\[{]/);
  if (first >= 0) starts.add(first);
  for (const match of text.matchAll(/\n\s*([\[{])/g)) {
    if (match.index != null) starts.add(match.index + match[0].lastIndexOf(match[1]));
  }
  for (const start of starts) {
    const end = balancedJsonEnd(text, start);
    if (end == null) continue;
    try {
      const count = collectionLength(JSON.parse(text.slice(start, end + 1)));
      if (count != null) return count;
    } catch {}
  }
  return undefined;
}

function resultCount(result: string | undefined, noun: string, lineFallback = false): string | undefined {
  if (!result?.trim()) return undefined;
  const count = embeddedCollectionLength(result);
  if (count != null) return `${count} ${noun}`;
  if (lineFallback) {
    const lines = result.split("\n").filter((line) => line.trim()).length;
    return lines > 1 ? `${lines} ${noun}` : undefined;
  }
  return undefined;
}

function targetTab(inv: CodetermInvocation, resolve?: TabNameResolver): string {
  const ref = flagText(inv, "tab", "tab") ?? inv.positionals[0];
  return ref ? resolve?.(ref) ?? `tab ${ref}` : "current tab";
}

function tabCloseObject(inv: CodetermInvocation, resolve?: TabNameResolver): string {
  const ref = flagText(inv, "tab", "tab") ?? inv.positionals[0];
  if (!ref) return "current tab";
  const resolved = resolve?.(ref);
  if (!resolved) return `tab ${ref}`;
  return resolved === `tab ${ref}` ? resolved : `${resolved} (${ref})`;
}

function commandObject(command: string | undefined): string | undefined {
  if (!command) return undefined;
  const port = command.match(/(?:port[ =:]|:)(\d{4,5})\b/i)?.[1] ?? command.match(/\b(7[0-9]{3})\b/)?.[1];
  if (/build\.(?:sh|ps1)\s+dev/i.test(command)) return port ? `dev @ ${port}` : "dev";
  return command.trim().split(/\s+/).slice(0, 3).join(" ");
}

function reportFields(inv: CodetermInvocation, result?: string): Array<{ label: string; value: string }> {
  const rawBody = resolvedReportText(result) ?? flagText(inv, "text") ?? inv.positionals.find((value) => /(?:^|\n|\\n)(?:STATUS|CHANGES|TESTS RUN|TESTS COVERING CHANGES|GATES REQUESTED|FOLLOWUP):/i.test(value));
  const body = rawBody ? decodeCliMessageText(rawBody) : undefined;
  const fields = new Map<string, string>();
  for (const line of body?.split(/\r?\n/) ?? []) {
    const match = /^(STATUS|CHANGES|TESTS RUN|TESTS COVERING CHANGES|GATES REQUESTED|FOLLOWUP):\s*(.*)$/i.exec(line.trim());
    if (match) fields.set(match[1].toLowerCase().replace(" run", ""), match[2]);
  }
  const status = flagText(inv, "status") ?? fields.get("status");
  const sha = flagText(inv, "sha");
  if (status) fields.set("status", status);
  if (sha) fields.set("sha", sha);
  return [...fields].map(([label, value]) => ({ label, value }));
}

/** Build the compact lead/object/chips payload used by the action card header; the object never repeats the lead. */
export function describeAction(inv: CodetermInvocation, result?: string, resolve?: TabNameResolver, resolvePeer?: PeerNameResolver, resolvePlugin?: PluginNameResolver, activityStatus?: string): ActionDescriptor {
  const descriptor = describeInvocation(inv, result, resolve, resolvePeer, resolvePlugin, activityStatus);
  const object = descriptor.object?.trim();
  return object && object.toLowerCase() !== descriptor.lead.trim().toLowerCase() ? descriptor : { ...descriptor, object: undefined };
}

function describeInvocation(inv: CodetermInvocation, result?: string, resolve?: TabNameResolver, resolvePeer?: PeerNameResolver, resolvePlugin?: PluginNameResolver, activityStatus?: string): ActionDescriptor {
  const path = inv.path.join(" ");
  if (isHelp(inv)) {
    return { lead: path, object: "help", chips: [] };
  }
  if (isSend(inv)) {
    const sent = parseSend(inv, result, resolve, resolvePeer);
    return {
      lead: "Sent to",
      object: formatCommunicationEndpoint(`${sent.recipient}${sent.mesh ? ` @ ${sent.mesh}` : ""}`),
      chips: sent.mesh ? ["mesh"] : ["tab"],
      detail: sent.body,
    };
  }
  if (path === "tab get-text") {
    const tab = targetTab(inv, resolve);
    return { lead: "tab get-text", object: tab, chips: [resultCount(result, "lines", true)].filter((v): v is string => Boolean(v)), fields: [{ label: "source", value: tab }] };
  }
  if (path === "tab new") {
    const command = flagText(inv, "cmd");
    return { lead: "tab new", object: commandObject(command) ?? "new tab", chips: ["new tab"], fields: command ? [{ label: "command", value: command }] : undefined };
  }
  if (path === "tab list") {
    const json = inv.flags.json === true;
    return json
      ? { lead: "tab list", object: "JSON inspection", chips: [resultCount(result, "tabs")].filter((v): v is string => Boolean(v)) }
      : { lead: "tab list", object: resultCount(result, "tabs") ?? "local workspace tabs", chips: ["compact"] };
  }
  if (path === "tab focus") {
    return { lead: "tab focus", object: targetTab(inv, resolve), chips: ["focused"] };
  }
  if (path === "tab close") {
    return { lead: "tab close", object: tabCloseObject(inv, resolve), chips: [] };
  }
  if (inv.path[0] === "tab") {
    return { lead: path, object: targetTab(inv, resolve), chips: [], detail: flagText(inv, "cmd", "query") };
  }
  if (path === "workspace focus") {
    return { lead: "workspace", object: firstMeaningful(inv) ?? "current", chips: ["focused"] };
  }
  if (path === "workspace list") {
    return { lead: "workspaces", object: resultCount(result, "workspaces") ?? "available", chips: [] };
  }
  if (inv.path[0] === "workspace") {
    return { lead: path, object: firstMeaningful(inv) ?? flagText(inv, "name") ?? "workspace", chips: [] };
  }
  if (path === "plan create") {
    return { lead: "plan", object: flagText(inv, "title") ?? "New plan", chips: [], fields: [{ label: "title", value: flagText(inv, "title") ?? "New plan" }] };
  }
  if (path === "plan goal set") {
    const plan = firstRef(PLAN_RE, inv, result) ?? "plan";
    return { lead: "goal", object: `set on ${plan}`, chips: [], fields: ["vector", "constraint", "postulate"].map((key) => ({ label: key, value: flagText(inv, key) ?? "—" })) };
  }
  if (inv.path[0] === "plan" && inv.path[1] === "task") {
    const action = inv.path[2];
    const task = taskActivityMetadata(inv, result, activityStatus);
    const target = action === "add" ? undefined : commandTaskDisplayRef(inv) ?? firstRef(TASK_RE, inv, result);
    const changingDeps = action === "update" && (inv.flags["clear-dependencies"] === true || inv.flags["depends-on"] != null);
    const lead = changingDeps ? "task deps" : action === "update" ? "task edit" : action === "close" ? "close" : action === "dispatch" ? "dispatch" : "plan task";
    const model = flagText(inv, "model");
    const chips = [model, task?.key, task?.status, action === "close" ? "done" : undefined].filter((value): value is string => Boolean(value));
    const fields = [
      ...(task?.key ? [{ label: "key", value: task.key }] : []),
      ...(task?.status ? [{ label: "status", value: task.status }] : []),
      ...(model ? [{ label: "model", value: model }] : []),
    ];
    const requestedName = action === "add" || action === "update" ? nonBlank(flagText(inv, "name")) : undefined;
    return {
      lead,
      object: task?.name ?? (taskMutationNameUnconfirmed(result, activityStatus) ? undefined : requestedName) ?? (action === "add" ? "New task" : target ?? "Task"),
      chips,
      detail: flagText(inv, "test-plan", "criterion", "correctness", "why"),
      fields: fields.length ? fields : undefined,
    };
  }
  if (inv.path[0] === "plan" && inv.path[1] === "section") {
    const section = firstRef(SECTION_RE, inv, result) ?? flagText(inv, "handle");
    return { lead: inv.path[2] === "update" ? "section edit" : "section", object: safeObject(flagText(inv, "title", "name"), section ?? "Section"), chips: section && !SECTION_RE.test(section) ? [section] : [], detail: flagText(inv, "body", "body-file") };
  }
  if (path === "plan record") {
    const task = firstRef(TASK_RE, inv, result);
    return { lead: "decision", object: task ? "on task" : firstRef(PLAN_RE, inv, result) ?? "plan", chips: [], detail: flagText(inv, "step", "name") };
  }
  if (path === "plan retrospective") return { lead: "retro", object: firstRef(PLAN_RE, inv, result) ?? "plan", chips: [], detail: flagText(inv, "body") ?? firstMeaningful(inv) };
  if (path === "plan state") return { lead: "state", object: firstRef(PLAN_RE, inv, result) ?? "plan", chips: [] };
  if (path === "plan skill new") return { lead: "skill", object: firstMeaningful(inv) ?? "new skill", chips: [] };
  if (inv.path[0] === "spec") {
    const spec = firstRef(SPEC_RE, inv, result);
    const isSection = inv.path[1] === "section";
    const isLink = inv.path[1] === "link";
    const from = flagText(inv, "spec-section");
    const to = flagText(inv, "plan-section");
    return { lead: isLink ? "link" : isSection ? "spec §" : "spec", object: isLink && (from || to) ? `${from ?? "spec"} → ${to ?? "plan"}` : safeObject(flagText(inv, "title", "name"), isLink ? "spec to plan" : isSection ? "Section" : "Spec"), chips: spec && !SPEC_RE.test(spec) ? [spec] : [], detail: flagText(inv, "body", "body-file"), fields: isLink ? [{ label: "from", value: from ?? "spec" }, { label: "to", value: to ?? "plan" }] : undefined };
  }
  if (inv.path[0] === "mem") {
    const memory = firstRef(MEMORY_RE, inv, result);
    return { lead: path, object: safeObject(flagText(inv, "title") ?? memory ?? firstMeaningful(inv), inv.path[1] === "wake-up" ? "identity + critical" : "Memory item"), chips: [flagText(inv, "kind") ?? resultCount(result, "hits")].filter((v): v is string => Boolean(v)), detail: firstMeaningful(inv) };
  }
  if (inv.path[0] === "agent") {
    const action = inv.path[1] ?? "action";
    const lead = action === "spawn" ? "spawn" : action === "list" ? "agents" : action;
    const provider = firstMeaningful(inv);
    const role = flagText(inv, "role");
    if (action === "report") {
      const task = firstRef(TASK_RE, inv, result);
      const status = flagText(inv, "status");
      const sha = flagText(inv, "sha");
      return {
        lead: "report",
        object: task ? `${task} · ${status ?? "report"}` : status ?? "report",
        chips: sha ? [sha] : [],
        fields: reportFields(inv, result),
      };
    }
    const object = action === "spawn" ? [role, provider].filter(Boolean).join(" · ") : action === "list" ? resultCount(result, "live") ?? "roster" : firstRef(TASK_RE, inv, result) ?? flagText(inv, "status") ?? provider;
    return { lead, object, chips: [flagText(inv, "model", "status")].filter((v): v is string => Boolean(v)), detail: flagText(inv, "task", "sha"), fields: action === "spawn" ? [{ label: "provider", value: provider ?? "default" }, { label: "model", value: flagText(inv, "model") ?? "default" }, { label: "role", value: role ?? "Worker" }] : undefined };
  }
  if (path === "mesh peers") {
    return { lead: "peers", object: resultCount(result, "online") ?? "mesh peers", chips: [] };
  }
  if (path === "cron create") {
    return { lead: "cron", object: flagText(inv, "schedule") ?? "scheduled", chips: ["active"], fields: [{ label: "schedule", value: flagText(inv, "schedule") ?? "—" }, { label: "task", value: flagText(inv, "task") ?? firstMeaningful(inv) ?? "—" }] };
  }
  if (path === "cron list") {
    return { lead: "crons", object: resultCount(result, "scheduled") ?? "scheduled jobs", chips: [] };
  }
  if (path === "keepawake start") {
    return { lead: "keepawake", object: "on", chips: ["holding"] };
  }
  if (path === "keepawake status") {
    return { lead: "status", object: result?.trim() || "keepawake", chips: [] };
  }
  if (path === "docs" || path === "doc") {
    const name = firstMeaningful(inv) ?? "documentation";
    const words = result?.trim().split(/\s+/).filter(Boolean).length;
    return { lead: "docs", object: name, chips: words ? [`${words} words`] : [] };
  }
  if (path === "plugin list") {
    return { lead: "plugins", object: resultCount(result, "installed") ?? "installed", chips: [] };
  }
  if (path === "orchestrator status") {
    return { lead: "agents", object: resultCount(result, "live") ?? "orchestration", chips: [], detail: flagText(inv, "plan") };
  }
  if (inv.path[0] === "plugin") return { ...describePluginAction(inv, resolvePlugin), chips: [] };
  if (inv.path[0] === "cron" || inv.path[0] === "keepawake" || inv.path[0] === "docs") {
    return { lead: path, object: firstMeaningful(inv) ?? flagText(inv, "schedule", "task", "name"), chips: [], detail: flagText(inv, "schedule", "task") };
  }
  const card = renderTemplate(inv, result, resolve);
  return { lead: path, object: card.title, chips: [], detail: card.detail };
}

/** Recover the (planId, taskId) a plan-family invocation points at. */
export function planLink(inv: CodetermInvocation, result?: string, activityStatus?: string): PlanLink | undefined {
  if (inv.path[0] === "plan" && inv.path[1] === "task" && (inv.path[2] === "add" || inv.path[2] === "update")) {
    const task = taskActivityMetadata(inv, result, activityStatus);
    const planId = commandPlanId(inv) ?? task?.planId;
    return task && planId ? { planId, taskId: task.id } : planId ? { planId } : undefined;
  }
  const planId =
    strFlag(inv, "plan") ??
    inv.positionals.find((p) => PLAN_RE.test(p)) ??
    firstMatch(PLAN_RE, result, inv.raw);
  if (!planId) return undefined;
  const taskId =
    strFlag(inv, "task") ??
    inv.positionals.find((p) => TASK_RE.test(p)) ??
    firstMatch(TASK_RE, result);
  return taskId ? { planId, taskId } : { planId };
}

function tab(inv: CodetermInvocation, resolve?: TabNameResolver): string {
  const p = inv.flags.tab ?? inv.flags.tab;
  if (typeof p !== "string") return "current Tab";
  return resolve?.(p) ?? p;
}

function sentTitle(inv: CodetermInvocation, resolve?: TabNameResolver): string {
  return `Sent to ${parseSend(inv, undefined, resolve).recipient}`;
}

function deliveredStatus(result?: string): CardStatus | undefined {
  if (!result) return undefined;
  try {
    const envelope = JSON.parse(result);
    const parsed = envelope.ok === false || envelope.error != null ? envelope : envelope.result;
    const delivery = parsed?.delivery ?? parsed?.status;
    if (parsed?.ok === false || parsed?.failed?.length || parsed?.error) {
      const error = parsed?.error;
      return { ok: false, label: "failed", detail: typeof error === "string" ? error : typeof error?.message === "string" ? error.message : undefined };
    }
    if (delivery === "submit_unconfirmed") return { ok: false, label: "submit_unconfirmed", pending: true };
    if (delivery === "interrupted") return { ok: false, label: "interrupted" };
    if (delivery === "unresolved") return { ok: false, label: "unresolved" };
    if (delivery === "error" || delivery === "failed") {
      const error = parsed?.error;
      return { ok: false, label: "failed", detail: typeof error === "string" ? error : typeof error?.message === "string" ? error.message : undefined };
    }
    if (delivery === "pending" || parsed?.pending?.length) return { ok: false, label: "pending", pending: true };
    if (delivery === "sent" || parsed?.sent?.length) return { ok: true, label: "sent" };
    if (delivery === "delivered" || parsed?.delivered?.length) return { ok: true, label: "delivered" };
    if (parsed?.ok === true) return { ok: false, label: "pending", pending: true };
  } catch {}
  if (/\bsubmit[_ -]unconfirmed\b/i.test(result)) return { ok: false, label: "submit_unconfirmed", pending: true };
  if (/\binterrupted\b/i.test(result)) return { ok: false, label: "interrupted" };
  if (/\bunresolved\b/i.test(result)) return { ok: false, label: "unresolved" };
  const explicitError = result.match(/^\s*(?:error|failed?|exit code:\s*[1-9]\d*)\b[^\r\n]*/im)?.[0]?.trim();
  if (explicitError) {
    const detail = explicitError.replace(/^(?:error|failed?)\s*:\s*/i, "").trim();
    return { ok: false, label: "failed", detail: (detail || explicitError).slice(0, 500) };
  }
  const statusLine = result.split(/\r?\n/).map((line) => line.trim()).find(Boolean);
  if (/^OK\s*\(pending\)$/i.test(statusLine ?? "")) {
    return { ok: false, label: "pending", pending: true };
  }
  if (/^(?:OK\s*\(delivered\)|delivered)$/i.test(statusLine ?? "")) return { ok: true, label: "delivered" };
  if (/^OK\s*\(sent\)$/i.test(statusLine ?? "")) return { ok: true, label: "sent" };
  if (/error|fail|not found/i.test(result)) {
    const detail = result.match(/^\s*(?:error|failed?)\s*:\s*([^\r\n]{1,500})/im)?.[1]
      ?? result.split(/\r?\n/).map((line) => line.trim()).find(Boolean)?.slice(0, 500);
    return { ok: false, label: "failed", detail };
  }
  return undefined;
}

export interface SentMessage {
  /** Resolved tab name, or `tab <ref>` / `current tab` fallback. */
  recipient: string;
  recipientId?: string;
  recipientTitleResolved: boolean;
  mesh?: string;
  body: string;
  bodySource: "resolved" | "typed" | "pad";
  truncated?: boolean;
  formattedAvailable: boolean;
  status?: CardStatus;
}

/** Parse a `codeterm send` invocation into an outgoing-message descriptor. */
export function parseSend(
  inv: CodetermInvocation,
  result?: string,
  resolve?: TabNameResolver,
  resolvePeer?: PeerNameResolver,
): SentMessage {
  const p = inv.flags.tab;
  const meshRef = typeof inv.flags.mesh === "string" ? inv.flags.mesh : undefined;
  const title = meshRef === undefined && typeof p === "string" ? resolve?.(p) : undefined;
  let recipient: string;
  if (typeof p !== "string") recipient = "current tab";
  else recipient = title?.trim() || `tab ${p}`;
  const mesh = meshRef ? friendlyPeerName(meshRef, resolvePeer) : undefined;
  const resolved = resolvedSendText(result);
  const typed = decodeCliMessageText(inv.positionals[0] ?? "");
  const fromPad = inv.flags["from-pad"];
  const fromFile = inv.flags["from-file"] ?? inv.flags.file;
  const staged =
    typeof fromPad === "string" ||
    typeof fromFile === "string" ||
    inv.flags.stdin === true;
  return {
    recipient,
    recipientId: typeof p === "string" ? p : undefined,
    recipientTitleResolved: Boolean(title?.trim()),
    mesh,
    body: resolved?.text || (staged ? sendBodyDescriptor(inv) : typed) || sendBodyDescriptor(inv),
    bodySource: resolved != null ? "resolved" : typeof fromPad === "string" ? "pad" : "typed",
    truncated: resolved?.truncated,
    formattedAvailable: resolved != null || !staged,
    status: deliveredStatus(result),
  };
}

function sendBodyDescriptor(inv: CodetermInvocation): string {
  const fromPad = inv.flags["from-pad"];
  if (typeof fromPad === "string") return `AgentPad payload: ${fromPad}`;
  const fromFile = inv.flags["from-file"] ?? inv.flags.file;
  if (typeof fromFile === "string") return `Message sent from file: ${fromFile}`;
  if (inv.flags.stdin === true) return "Message sent from standard input.";
  return "Message content unavailable.";
}

/** The body the CLI actually delivered, echoed by every staged delivery path. */
export function resolvedReportText(result?: string): string | undefined {
  const encoded = result?.match(/^CODETERM_RESOLVED_TEXT=(.+)$/m)?.[1];
  if (!encoded) return undefined;
  try {
    const text = JSON.parse(encoded);
    return typeof text === "string" ? text : undefined;
  } catch {
    return undefined;
  }
}

function resolvedSendText(result?: string): { text: string; truncated: boolean } | undefined {
  const encoded = result?.match(/^CODETERM_RESOLVED_TEXT=(.+)$/m)?.[1];
  if (encoded) {
    try {
      const text = JSON.parse(encoded);
      if (typeof text === "string") return { text: text.slice(0, 16_384), truncated: text.length > 16_384 || /CODETERM_RESOLVED_TEXT_TRUNCATED=true/.test(result ?? "") };
    } catch {}
  }
  try {
    const evidence = JSON.parse(result ?? "")?.result?.sent_message;
    if (typeof evidence?.text === "string") return { text: evidence.text.slice(0, 16_384), truncated: evidence.truncated === true || evidence.text.length > 16_384 };
  } catch {}
  return undefined;
}

/** True when this invocation is an outgoing `codeterm send`. */
export function isSend(inv: CodetermInvocation): boolean {
  return inv.path.length === 1 && inv.path[0] === "send" && !isHelp(inv);
}

export function isHelp(inv: CodetermInvocation): boolean {
  return inv.flags.help === true || inv.flags.h === true;
}

type TemplateFn = (inv: CodetermInvocation, result?: string, resolve?: TabNameResolver) => RenderedCard;

const sendTextTemplate: TemplateFn = (inv, result, resolve) => {
  const mesh = typeof inv.flags.mesh === "string" ? inv.flags.mesh : undefined;
  return {
    icon: mesh ? "🛰" : "→",
    title: mesh ? `${sentTitle(inv, resolve)} @ ${mesh}` : sentTitle(inv, resolve),
    detail: inv.positionals[0],
    status: deliveredStatus(result),
  };
};

const planTaskTemplate: TemplateFn = (inv, result) => {
  const action = inv.path[2] ?? inv.positionals[0];
  const link = planLink(inv, result);
  const task = taskActivityMetadata(inv, result);
  if ((action === "add" || action === "update") && taskMutationFailed(result)) {
    return { icon: "🗂", title: action === "update" ? "Task update failed" : "Task creation failed", link };
  }
  if (action === "update") {
    return { icon: "🔄", title: task?.name ? `Task updated: ${task.name}` : "Task updated", link };
  }
  if (action === "add") {
    return { icon: "🗂", title: task?.name ? `Task created: ${task.name}` : "Task created", link };
  }
  return { icon: "🗂", title: `Plan task ${action ?? ""}`.trim(), link };
};

const planRecordTemplate: TemplateFn = (inv, result) => {
  const status = strFlag(inv, "status");
  const note = flagText(inv, "step", "name");
  const firstLine = note ? note.split("\n")[0] : undefined;
  return {
    icon: "📝",
    title: status ? `Progress recorded → ${status}` : "Progress recorded",
    detail: firstLine,
    link: planLink(inv, result),
  };
};

const planStateTemplate: TemplateFn = (inv, result) => ({
  icon: "🗺",
  title: "Plan state",
  link: planLink(inv, result),
});

const TEMPLATES: Record<string, TemplateFn> = {
  send: sendTextTemplate,
  "plan task": planTaskTemplate,
  "plan record": planRecordTemplate,
  "plan state": planStateTemplate,
  "agent spawn": (inv) => ({
    icon: "🤖",
    title: `Spawned ${inv.positionals[0] ?? "claude"}`,
    detail: typeof inv.flags.task === "string" ? inv.flags.task : undefined,
  }),
  "agent report": (inv) => ({ icon: "📨", title: "Reported to parent", detail: inv.positionals[0] }),
  "agent providers": () => ({ icon: "🤖", title: "Listed agent providers" }),
  "mem save": (inv) => ({
    icon: "🧠",
    title: `Saved memory${typeof inv.flags.title === "string" ? `: ${inv.flags.title}` : ""}`,
    detail: inv.positionals[0],
  }),
  "mem search": (inv) => ({ icon: "🔍", title: "Searched mem", detail: inv.positionals[0] }),
  "mem get": (inv) => ({ icon: "📖", title: `Recalled ${inv.positionals[0] ?? ""}` }),
  "workspace create": (inv) => ({ icon: "🪟", title: `Created workspace ${inv.positionals[0] ?? ""}` }),
  "tab focus": (inv, _result, resolve) => {
    const ref = inv.positionals[0] ?? (typeof inv.flags.tab === "string" ? inv.flags.tab : undefined);
    const name = ref ? resolve?.(ref) : undefined;
    return { icon: "→", title: `Focused ${name ?? `tab ${ref ?? tab(inv, resolve)}`}` };
  },
  "cron create": (inv) => ({ icon: "⏰", title: `Scheduled ${inv.positionals[0] ?? "job"}` }),
  "keepawake start": () => ({ icon: "☕", title: "Keep-awake started" }),
};

/** Pick the longest matching template, else a branded fallback. */
export function renderTemplate(
  inv: CodetermInvocation,
  result?: string,
  resolve?: TabNameResolver,
): RenderedCard {
  const two = inv.path.slice(0, 2).join(" ");
  const one = inv.path[0];
  const fn = TEMPLATES[two] ?? TEMPLATES[one];
  if (fn) return fn(inv, result, resolve);
  return {
    icon: "⬡",
    title: inv.path.join(" "),
    detail: inv.positionals.join(" ") || undefined,
    status: deliveredStatus(result),
  };
}
