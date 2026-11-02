import type { CodetermInvocation } from "./recognize";
import { parseSend, type CardStatus, type PeerNameResolver, type TabNameResolver } from "./templates";
import type { OpenPlan, ResolvedChatPlanRef } from "./LinkedText";
import { useId, useState } from "react";
import { InboundActivityCard } from "./InboundActivityCard";
import { CompactMessageContent } from "./CompactMessageContent";
import { RadioTower, Send } from "lucide-react";

interface Props {
  inv: CodetermInvocation;
  eventId?: string;
  onFocusTab?: () => void;
  result?: string;
  resolveTabName?: TabNameResolver;
  resolvePeerName?: PeerNameResolver;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: "plan" | "task" | "section") => ResolvedChatPlanRef | null;
  fontScale?: number;
  technicalCommand?: string;
  activityStatus?: "pending" | "success" | "error" | "unresolved" | "interrupted" | "running" | "submit_unconfirmed";
}

function resultError(result?: string): string | undefined {
  const lines = result?.split(/\r?\n/).map((line) => line.trim()).filter(Boolean) ?? [];
  return (lines.find((line) => /^(?:error|failed?|exit code:\s*[1-9]\d*)\b/i.test(line)) ?? lines[0])?.slice(0, 500);
}

function activityStatusValue(status: Props["activityStatus"], result?: string): CardStatus | undefined {
  if (status === "pending" || status === "running") return { ok: false, label: "pending", pending: true };
  if (status === "submit_unconfirmed") return { ok: false, label: "submit_unconfirmed", pending: true };
  if (status === "error") return { ok: false, label: "failed", detail: resultError(result) };
  if (status === "unresolved") return { ok: false, label: "unresolved" };
  if (status === "interrupted") return { ok: false, label: "interrupted" };
  return undefined;
}

export function SentMessageBubble({ inv, result, resolveTabName, resolvePeerName, onOpenPlan, resolvePlanRef, fontScale, technicalCommand, activityStatus, onFocusTab, eventId }: Props) {
  const parsed = parseSend(inv, result, resolveTabName, resolvePeerName);
  const activity = activityStatusValue(activityStatus, result);
  const deliveryStatus = parsed.status?.label === "failed"
    ? { ...parsed.status, detail: parsed.status.detail ?? (activity?.label === "failed" ? activity.detail : undefined) }
    : activity?.label === "failed"
      ? { ...activity, detail: parsed.status?.detail ?? activity.detail }
      : activity ?? parsed.status;
  const msg = { ...parsed, status: deliveryStatus };
  const generatedId = useId();
  const [expanded, setExpanded] = useState(true);
  const cardStatus = msg.status?.pending ? "pending"
    : msg.status?.ok ? "success"
      : msg.status?.label === "interrupted" ? "interrupted"
        : msg.status?.label === "unresolved" ? "unresolved"
          : msg.status ? "error"
            : activityStatus === "error" ? "error"
              : activityStatus === "pending" || activityStatus === "running" ? "pending"
                : activityStatus === "interrupted" ? "interrupted"
                  : "unresolved";
  return <InboundActivityCard
    testId="chat-sent-to-card"
    headerTestId="chat-sent-header"
    statusTestId="chat-sent-status"
    statusLabel={msg.status?.label ?? (cardStatus === "unresolved" ? "unresolved" : undefined)}
    message={{ id: eventId ?? generatedId, type: "user", content: msg.body }}
    title={`Sent to ${msg.recipient}${msg.mesh ? ` @ ${msg.mesh}` : ""}`}
    subtitle={msg.recipientTitleResolved && msg.recipientId ? `Tab ID ${msg.recipientId}` : undefined}
    summary=""
    status={cardStatus}
    visual={{ glyph: msg.mesh ? RadioTower : Send, color: "var(--ct-accent)", label: "sent" }}
    expanded={expanded}
    onToggle={() => setExpanded(value => !value)}
    onFocusTab={onFocusTab}
    technicalCommand={technicalCommand}
    showTechnicalDetails={Boolean(technicalCommand)}
    focusTabTestId="codeterm-focus-tab"
    disclosureLabel="sent message">
    <div data-testid="chat-sent-message">
      <CompactMessageContent
        text={msg.body}
        onOpenPlan={onOpenPlan}
        resolvePlanRef={resolvePlanRef}
        fontScale={fontScale}
        expandTestId="chat-sent-expand"
      />
      {msg.status?.label === "failed" && msg.status.detail && <p role="alert" className="mt-1 text-xs" style={{ color: "var(--ct-err, #ff6b6b)" }}>{msg.status.detail.slice(0, 500)}</p>}
      {msg.truncated && <p role="note" className="mt-1 text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>Message preview capped at 16 KB.</p>}
    </div>
  </InboundActivityCard>;
}
