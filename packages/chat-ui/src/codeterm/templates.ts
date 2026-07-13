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

/** Resolve a pane id/short-id to a human tab name. Returns undefined when unknown. */
export type PaneNameResolver = (paneRef: string) => string | undefined;

const TASK_RE = /task_[0-9a-f]{32}/;
const PLAN_RE = /pl_[0-9a-f]{32}/;

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

/** Recover the (planId, taskId) a plan-family invocation points at. */
export function planLink(inv: CodetermInvocation, result?: string): PlanLink | undefined {
  const planId =
    strFlag(inv, "plan") ??
    inv.positionals.find((p) => PLAN_RE.test(p)) ??
    firstMatch(PLAN_RE, result, inv.raw);
  if (!planId) return undefined;
  const taskId =
    strFlag(inv, "task") ??
    inv.positionals.find((p) => /^task_[0-9a-f]{32}$/.test(p)) ??
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
