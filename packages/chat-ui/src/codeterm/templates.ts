import type { CodetermInvocation } from "./recognize";
import { decodeCliMessageText } from "./messageText";

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
    const candidate = inv.flags.tab ?? inv.flags.tab;
    const ref = typeof candidate === "string" ? candidate : undefined;
    return ref ? { kind: "tab", id: ref } : null;
  }
  if (inv.path[0] === "tab") {
    const candidate = inv.flags.tab ?? inv.flags.tab ?? inv.positionals[0];
    return typeof candidate === "string" ? { kind: "tab", id: candidate } : null;
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
  if (Array.isArray(value)) return value.length;
  if (!value || typeof value !== "object") return undefined;
  const record = value as Record<string, unknown>;
  for (const key of ["items", "tabs", "workspaces", "results", "data"]) {
    if (Array.isArray(record[key])) return record[key].length;
  }
  return undefined;
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

function reportFields(inv: CodetermInvocation): Array<{ label: string; value: string }> {
  const rawBody = flagText(inv, "text") ?? inv.positionals.find((value) => /(?:^|\n|\\n)(?:STATUS|CHANGES|TESTS RUN|GATES REQUESTED|FOLLOWUP):/i.test(value));
  const body = rawBody ? decodeCliMessageText(rawBody) : undefined;
  const fields = new Map<string, string>();
  for (const line of body?.split(/\r?\n/) ?? []) {
    const match = /^(STATUS|CHANGES|TESTS RUN|GATES REQUESTED|FOLLOWUP):\s*(.*)$/i.exec(line.trim());
    if (match) fields.set(match[1].toLowerCase().replace(" run", ""), match[2]);
  }
  const status = flagText(inv, "status") ?? fields.get("status");
  const sha = flagText(inv, "sha");
  if (status) fields.set("status", status);
  if (sha) fields.set("sha", sha);
  return [...fields].map(([label, value]) => ({ label, value }));
}

/** Build the compact lead/object/chips payload used by the action card header. */
export function describeAction(inv: CodetermInvocation, result?: string, resolve?: TabNameResolver, resolvePeer?: PeerNameResolver): ActionDescriptor {
  const path = inv.path.join(" ");
  if (isHelp(inv)) {
    return { lead: "help", object: path, chips: [] };
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
      : { lead: "tab list", object: resultCount(result, "tabs") ?? "workspace tabs", chips: ["compact"] };
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
    const task = firstRef(TASK_RE, inv, result);
    const name = flagText(inv, "name", "title") ?? firstMeaningful(inv, "task", "add", "update", "close");
    const action = inv.path[2];
    const changingDeps = action === "update" && (inv.flags["clear-dependencies"] === true || inv.flags["depends-on"] != null);
    const lead = changingDeps ? "task deps" : action === "update" ? "task edit" : action === "close" ? "close" : action === "dispatch" ? "dispatch" : "plan task";
    const model = flagText(inv, "model");
    return { lead, object: safeObject(name, task ? "Task" : "New task"), chips: [model, action === "close" ? "done" : undefined].filter((v): v is string => Boolean(v)), detail: flagText(inv, "test-plan", "criterion", "correctness", "why"), fields: model ? [{ label: "model", value: model }] : undefined };
  }
  if (inv.path[0] === "plan" && inv.path[1] === "section") {
    const section = firstRef(SECTION_RE, inv, result) ?? flagText(inv, "handle");
    return { lead: inv.path[2] === "update" ? "section edit" : "section", object: safeObject(flagText(inv, "title", "name"), section ?? "Section"), chips: section && !SECTION_RE.test(section) ? [section] : [], detail: flagText(inv, "body", "body-file") };
  }
  if (path === "plan record") {
    const task = firstRef(TASK_RE, inv, result);
    return { lead: "decision", object: task ? "on task" : firstRef(PLAN_RE, inv, result) ?? "plan", chips: [], detail: firstMeaningful(inv) ?? flagText(inv, "step") };
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
        fields: reportFields(inv),
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

function tab(inv: CodetermInvocation, resolve?: TabNameResolver): string {
  const p = inv.flags.tab ?? inv.flags.tab;
  if (typeof p !== "string") return "current Tab";
  return resolve?.(p) ?? p;
}

function sentTitle(inv: CodetermInvocation, resolve?: TabNameResolver): string {
  const p = inv.flags.tab ?? inv.flags.tab;
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
  bodySource: "resolved" | "typed" | "pad";
  status?: CardStatus;
}

/** Parse a `codeterm send` invocation into an outgoing-message descriptor. */
export function parseSend(
  inv: CodetermInvocation,
  result?: string,
  resolve?: TabNameResolver,
  resolvePeer?: PeerNameResolver,
): SentMessage {
  const p = inv.flags.tab ?? inv.flags.tab;
  let recipient: string;
  if (typeof p !== "string") recipient = "current tab";
  else recipient = resolve?.(p) ?? `tab ${p}`;
  const meshRef = typeof inv.flags.mesh === "string" ? inv.flags.mesh : undefined;
  const mesh = meshRef ? friendlyPeerName(meshRef, resolvePeer) : undefined;
  const resolved = resolvedSendText(result);
  const typed = decodeCliMessageText(inv.positionals[0] ?? "");
  const fromPad = inv.flags["from-pad"];
  const staged =
    typeof fromPad === "string" ||
    typeof inv.flags["from-file"] === "string" ||
    inv.flags.stdin === true;
  return {
    recipient,
    mesh,
    body: resolved || (staged ? sendBodyDescriptor(inv) : typed) || sendBodyDescriptor(inv),
    bodySource: resolved != null ? "resolved" : typeof fromPad === "string" ? "pad" : "typed",
    status: deliveredStatus(result),
  };
}

function sendBodyDescriptor(inv: CodetermInvocation): string {
  const fromPad = inv.flags["from-pad"];
  if (typeof fromPad === "string") return `AgentPad payload: ${fromPad}`;
  const fromFile = inv.flags["from-file"];
  if (typeof fromFile === "string") return `Message sent from file: ${fromFile}`;
  if (inv.flags.stdin === true) return "Message sent from standard input.";
  return "Message content unavailable.";
}

function resolvedSendText(result?: string): string | undefined {
  const encoded = result?.match(/^CODETERM_RESOLVED_TEXT=(.+)$/m)?.[1];
  if (!encoded) return undefined;
  try {
    const text = JSON.parse(encoded);
    return typeof text === "string" ? text : undefined;
  } catch {
    return undefined;
  }
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
  if (action === "update") {
    const status = strFlag(inv, "status");
    return { icon: "🔄", title: status ? `Task status → ${status}` : "Task updated", link };
  }
  if (action === "add") {
    const name = strFlag(inv, "name");
    return { icon: "🗂", title: name ? `Task created: ${name}` : "Task created", link };
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
