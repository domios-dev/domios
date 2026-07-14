import type { CodetermInvocation } from "./recognize";

export interface CardStatus {
  ok: boolean;
  label: string;
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
}

/** Resolve a pane id/short-id to a human tab name. Returns undefined when unknown. */
export type PaneNameResolver = (paneRef: string) => string | undefined;

export type NavigationTarget =
  | { kind: "tab"; id: string }
  | { kind: "plan" | "task" | "section" | "spec" | "goal" | "memory" | "source"; id: string; planId?: string };

const TASK_RE = /task_[a-z0-9][a-z0-9_-]*/i;
const PLAN_RE = /pl_[a-z0-9][a-z0-9_-]*/i;
const SECTION_RE = /sec_[a-z0-9][a-z0-9_-]*/i;
const SPEC_RE = /(?:sp|spec)_[a-z0-9][a-z0-9_-]*/i;
const MEMORY_RE = /mem_[a-z0-9][a-z0-9_-]*/i;
const SOURCE_RE = /src_[a-z0-9][a-z0-9_-]*/i;

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

/** Describe the stable navigation target represented by a recognized invocation. */
export function navigationTarget(inv: CodetermInvocation, result?: string): NavigationTarget | null {
  if (isSend(inv)) {
    const candidate = inv.flags.tab ?? inv.flags.pane;
    const ref = typeof candidate === "string" ? candidate : undefined;
    return ref ? { kind: "tab", id: ref } : null;
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
  if (inv.path[0] === "plan" && inv.positionals[0] === "goal" && planId) return { kind: "goal", id: planId, planId };
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

/** Build the compact lead/object/chips payload used by the action card header. */
export function describeAction(inv: CodetermInvocation, result?: string, resolve?: PaneNameResolver): ActionDescriptor {
  const path = inv.path.join(" ");
  if (isSend(inv)) {
    const sent = parseSend(inv, result, resolve);
    return { lead: "send", object: sent.recipient, chips: sent.mesh ? [sent.mesh] : [], detail: sent.body };
  }
  if (path === "plan goal" || (inv.path[0] === "plan" && inv.positionals[0] === "goal")) {
    return { lead: "goal", object: "Goal", chips: [], detail: flagText(inv, "vector", "vision") };
  }
  if (inv.path[0] === "plan" && (inv.path[1] === "task" || inv.positionals[0] === "task")) {
    const task = firstRef(TASK_RE, inv, result);
    const name = flagText(inv, "name", "title") ?? firstMeaningful(inv, "task", "add", "update", "close");
    return { lead: "plan task", object: safeObject(name, "Task"), chips: [task ? "task" : ""].filter(Boolean), detail: flagText(inv, "test-plan", "criterion", "correctness", "why") };
  }
  if (inv.path[0] === "plan" && (inv.path[1] === "section" || inv.positionals[0] === "section")) {
    const section = firstRef(SECTION_RE, inv, result) ?? flagText(inv, "handle");
    return { lead: "section", object: safeObject(flagText(inv, "title", "name"), "Section"), chips: section && !SECTION_RE.test(section) ? [section] : [], detail: flagText(inv, "body", "body-file") };
  }
  if (inv.path[0] === "spec") {
    const spec = firstRef(SPEC_RE, inv, result);
    const isSection = inv.path[1] === "section" || inv.positionals[0] === "section";
    return { lead: isSection ? "spec §" : "spec", object: safeObject(flagText(inv, "title", "name"), isSection ? "Section" : "Spec"), chips: spec && !SPEC_RE.test(spec) ? [spec] : [], detail: flagText(inv, "body", "body-file") };
  }
  if (inv.path[0] === "mem") {
    const memory = firstRef(MEMORY_RE, inv, result);
    return { lead: `mem ${inv.path[1] ?? "action"}`, object: safeObject(flagText(inv, "title") ?? memory ?? firstMeaningful(inv), "Memory item"), chips: [flagText(inv, "kind") ?? ""].filter(Boolean), detail: firstMeaningful(inv) };
  }
  if (inv.path[0] === "agent") {
    return { lead: `agent ${inv.path[1] ?? "action"}`, object: firstMeaningful(inv) ?? flagText(inv, "role", "model"), chips: [flagText(inv, "model", "status") ?? ""].filter(Boolean), detail: flagText(inv, "task", "sha") };
  }
  if (inv.path[0] === "tab" || inv.path[0] === "workspace") {
    return { lead: path, object: firstMeaningful(inv) ?? flagText(inv, "cmd", "name"), chips: [], detail: flagText(inv, "cmd") };
  }
  if (inv.path[0] === "cron" || inv.path[0] === "keepawake" || inv.path[0] === "plugin" || inv.path[0] === "docs") {
    return { lead: path, object: firstMeaningful(inv) ?? flagText(inv, "schedule", "task", "name"), chips: [], detail: flagText(inv, "schedule", "task") };
  }
  const card = renderTemplate(inv, result, resolve);
  return { lead: path, object: card.title, chips: [], detail: card.detail };
}

/** Recover the (planId, taskId) a plan-family invocation points at. */
export function planLink(inv: CodetermInvocation, result?: string): PlanLink | undefined {
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

function pane(inv: CodetermInvocation, resolve?: PaneNameResolver): string {
  const p = inv.flags.tab ?? inv.flags.pane;
  if (typeof p !== "string") return "current pane";
  return resolve?.(p) ?? p;
}

function sentTitle(inv: CodetermInvocation, resolve?: PaneNameResolver): string {
  const p = inv.flags.tab ?? inv.flags.pane;
  if (typeof p !== "string") return "Sent to current tab";
  const name = resolve?.(p);
  return name ? `Sent to ${name}` : `Sent to tab ${p}`;
}

function deliveredStatus(result?: string): CardStatus | undefined {
  if (!result) return undefined;
  if (/\bOK\b|\(sent\)|delivered/i.test(result)) return { ok: true, label: "delivered" };
  if (/error|fail|not found/i.test(result)) return { ok: false, label: "failed" };
  return undefined;
}

export interface SentMessage {
  /** Resolved tab name, or `tab <ref>` / `current tab` fallback. */
  recipient: string;
  mesh?: string;
  body: string;
  status?: CardStatus;
}

/** Parse a `codeterm send` invocation into an outgoing-message descriptor. */
export function parseSend(
  inv: CodetermInvocation,
  result?: string,
  resolve?: PaneNameResolver,
): SentMessage {
  const p = inv.flags.tab ?? inv.flags.pane;
  let recipient: string;
  if (typeof p !== "string") recipient = "current tab";
  else recipient = resolve?.(p) ?? `tab ${p}`;
  const mesh = typeof inv.flags.mesh === "string" ? inv.flags.mesh : undefined;
  return { recipient, mesh, body: inv.positionals[0] ?? "", status: deliveredStatus(result) };
}

/** True when this invocation is an outgoing `codeterm send`. */
export function isSend(inv: CodetermInvocation): boolean {
  return inv.path.length === 1 && inv.path[0] === "send";
}

type TemplateFn = (inv: CodetermInvocation, result?: string, resolve?: PaneNameResolver) => RenderedCard;

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
  const action = inv.positionals[0];
  const link = planLink(inv, result);
  if (action === "update") {
    const status = strFlag(inv, "status");
    return { icon: "🔄", title: status ? `Task status → ${status}` : "Task updated", link };
  }
  if (action === "add") {
    const name = strFlag(inv, "title");
    return { icon: "🗂", title: name ? `Task created: ${name}` : "Task created", link };
  }
  return { icon: "🗂", title: `Plan task ${action ?? ""}`.trim(), link };
};

const planRecordTemplate: TemplateFn = (inv, result) => {
  const kind = strFlag(inv, "kind");
  const note = inv.positionals[0] ?? strFlag(inv, "note");
  const firstLine = note ? note.split("\n")[0] : undefined;
  return {
    icon: "📝",
    title: kind ? `Plan updated: ${kind}` : "Plan updated",
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
    return { icon: "→", title: `Focused ${name ?? `tab ${ref ?? pane(inv, resolve)}`}` };
  },
  "cron create": (inv) => ({ icon: "⏰", title: `Scheduled ${inv.positionals[0] ?? "job"}` }),
  "keepawake start": () => ({ icon: "☕", title: "Keep-awake started" }),
};

/** Pick the longest matching template, else a branded fallback. */
export function renderTemplate(
  inv: CodetermInvocation,
  result?: string,
  resolve?: PaneNameResolver,
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
