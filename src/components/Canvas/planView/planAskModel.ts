import type { PlanView as Plan } from "../planProjection";
import { tabActivity } from "../../../lib/tabActivity";
import type { Tab, TabStatus } from "../../../lib/types";
import { ownerName, ownerTabId } from "../owner";
import { resolveCompanionTab } from "./companionResolver";

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

export type PlanReviewAction = "comment" | "request_change";

export type PlanAskOwner =
  | PlanAskReadyOwner
  | { status: "missing" }
  | { status: "offline"; tabId: string; name: string };

const FALLBACK_OWNER_NAME = "Orchestrator";

/** Only the plan's recorded owner tab is ever a destination; an absent or agentless tab is reported, never substituted. */
export function resolvePlanAskOwner(plan: Pick<Plan, "owner">, tabs: Record<string, Tab> | undefined, statuses?: Record<string, TabStatus>): PlanAskOwner {
  const tabId = ownerTabId(plan.owner);
  if (!tabId) return { status: "missing" };
  return resolvePlanAskRecipient(tabId, ownerName(plan.owner) || FALLBACK_OWNER_NAME, tabs, statuses);
}

export function resolvePlanReviewOwner(
  plan: Pick<Plan, "owner" | "planner">,
  tabs: Record<string, Tab> | undefined,
  statuses?: Record<string, TabStatus>,
): PlanAskOwner {
  const recipient = resolveCompanionTab(plan, (tabId) => {
    const tab = tabs?.[tabId];
    return Boolean(tab && !tab.hidden && (tab.agent_type || tab.backing?.kind === "chat_backend"));
  });
  if (recipient?.kind === "planner") {
    const name = plan.planner?.kind === "tab" ? plan.planner.title?.trim() || "Planner" : "Planner";
    return resolvePlanAskRecipient(recipient.tabId, name, tabs, statuses);
  }
  if (recipient?.kind === "orchestrator") return resolvePlanAskOwner(plan, tabs, statuses);

  if (plan.planner?.kind === "tab") {
    return resolvePlanAskRecipient(plan.planner.tab_id, plan.planner.title?.trim() || "Planner", tabs, statuses);
  }
  return resolvePlanAskOwner(plan, tabs, statuses);
}

function resolvePlanAskRecipient(
  tabId: string,
  fallbackName: string,
  tabs: Record<string, Tab> | undefined,
  statuses?: Record<string, TabStatus>,
): PlanAskOwner {
  const tab = tabs?.[tabId];
  const name = tab?.title?.trim() || fallbackName;
  const chatBackend = tab?.backing?.kind === "chat_backend";
  if (!tab || (!tab.agent_type && !chatBackend)) return { status: "offline", tabId, name };
  return {
    status: "ready",
    tabId,
    name,
    busy: tabActivity(tab, statuses?.[tabId]).active,
    chatBackend,
    agentSessionId: tab.agent_session_id ?? null,
    sessionBindingNonce: tab.session_binding_nonce ?? null,
  };
}

function createPlanMessageId(kind: "ask" | "review"): string {
  const random = globalThis.crypto?.randomUUID?.() ?? `${Date.now().toString(36)}${Math.random().toString(36).slice(2)}`;
  return `plan-${kind}-${random}`;
}

export function createPlanAskId(): string {
  return createPlanMessageId("ask");
}

export function createPlanReviewId(): string {
  return createPlanMessageId("review");
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

export function composePlanReviewMessage(
  reference: PlanAskReference & { mode: "selection" },
  action: PlanReviewAction,
  body: string,
  reviewId: string,
): string {
  const label = action === "comment" ? "Comment" : "Request change";
  const payload = {
    review_id: reviewId,
    kind: "plan_review",
    plan_id: reference.planId,
    document: reference.document,
    spec_id: reference.specId,
    item_count: 1,
    items: [{
      action,
      body: body.trim(),
      mode: reference.mode,
      section_id: reference.sectionId,
      section_title: reference.sectionTitle,
      source_start: reference.source.start,
      source_end: reference.source.end,
      rendered_start: reference.rendered.start,
      rendered_end: reference.rendered.end,
      offset_unit: PLAN_ASK_OFFSET_UNIT,
      quote: reference.quote,
    }],
  };
  const document = reference.document === "spec" ? "Spec" : "Plan";
  return [
    `One-item plan review: ${label}`,
    `${document} section ${reference.sectionTitle} (${reference.sectionId}) in plan ${reference.planId}:`,
    body.trim(),
    "",
    "Review reference:",
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
