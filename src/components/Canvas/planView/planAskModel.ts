import type { Tab } from "../../../lib/types";
import { ownerName, ownerTabId } from "../owner";
import type { Plan } from "../types";
import type { PlanAskReference } from "./planAskMapping";

export interface PlanAskReadyOwner {
  status: "ready";
  tabId: string;
  name: string;
  busy: boolean;
  chatBackend: boolean;
  agentSessionId: string | null;
  sessionBindingNonce: string | null;
}

export type PlanAskOwner =
  | PlanAskReadyOwner
  | { status: "missing" }
  | { status: "offline"; tabId: string; name: string };

const FALLBACK_OWNER_NAME = "Orchestrator";

/** Only the plan's recorded owner tab is ever a destination; an absent or agentless tab is reported, never substituted. */
export function resolvePlanAskOwner(plan: Pick<Plan, "owner">, tabs: Record<string, Tab> | undefined): PlanAskOwner {
  const tabId = ownerTabId(plan.owner);
  if (!tabId) return { status: "missing" };
  const tab = tabs?.[tabId];
  const name = tab?.title?.trim() || ownerName(plan.owner) || FALLBACK_OWNER_NAME;
  const chatBackend = tab?.backing?.kind === "chat_backend";
  if (!tab || (!tab.agent_type && !chatBackend)) return { status: "offline", tabId, name };
  return {
    status: "ready",
    tabId,
    name,
    busy: tab.agent_state?.state === "working",
    chatBackend,
    agentSessionId: tab.agent_session_id ?? null,
    sessionBindingNonce: tab.session_binding_nonce ?? null,
  };
}

export function createPlanAskId(): string {
  const random = globalThis.crypto?.randomUUID?.() ?? `${Date.now().toString(36)}${Math.random().toString(36).slice(2)}`;
  return `plan-ask-${random}`;
}

export const PLAN_ASK_OFFSET_UNIT = "utf16_code_unit";

/** The last line is a single-line JSON reference so the quote survives byte-for-byte. */
export function composePlanAskMessage(reference: PlanAskReference, question: string, askId: string): string {
  const payload = {
    ask_id: askId,
    plan_id: reference.planId,
    document: reference.document,
    spec_id: reference.specId,
    section_id: reference.sectionId,
    section_title: reference.sectionTitle,
    mode: reference.mode,
    source_start: reference.source.start,
    source_end: reference.source.end,
    rendered_start: reference.rendered.start,
    rendered_end: reference.rendered.end,
    offset_unit: PLAN_ASK_OFFSET_UNIT,
    quote: reference.quote,
  };
  const document = reference.document === "spec" ? "Spec" : "Plan";
  return [
    `Question about ${document} text in plan ${reference.planId}, section ${reference.sectionId}:`,
    question,
    "",
    "Reference:",
    JSON.stringify(payload),
  ].join("\n");
}

interface ExchangeMessage {
  type: string;
  content: string;
  correlationId?: string;
  inboundKind?: string | null;
}

/** The exchange starts at the user turn carrying this ask id and ends at the next turn the user typed. */
export function planAskReplies<T extends ExchangeMessage>(
  messages: readonly T[],
  askId: string,
): { asked: boolean; replies: T[] } {
  const start = messages.findIndex(
    (message) => message.type === "user" && (message.correlationId === askId || message.content.includes(askId)),
  );
  if (start < 0) return { asked: false, replies: [] };
  const replies: T[] = [];
  for (const message of messages.slice(start + 1)) {
    if (message.type === "user" && !message.inboundKind) break;
    if (message.type === "assistant" && message.content.trim()) replies.push(message);
  }
  return { asked: true, replies };
}
