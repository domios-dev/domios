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
  activityStatus?: "pending" | "success" | "error" | "unresolved" | "interrupted" | "running";
}

export function SentMessageBubble({ inv, result, resolveTabName, resolvePeerName, onOpenPlan, resolvePlanRef, fontScale, technicalCommand, activityStatus, onFocusTab, eventId }: Props) {
  const parsed = parseSend(inv, result, resolveTabName, resolvePeerName);
  const activity: CardStatus | undefined = activityStatus === "pending" || activityStatus === "running"
    ? { ok: false, label: "pending", pending: true }
    : activityStatus === "success" ? { ok: true, label: "sent" }
      : activityStatus === "error" ? { ok: false, label: "failed" }
        : activityStatus === "unresolved" || activityStatus === "interrupted" ? { ok: false, label: "unconfirmed", pending: true }
          : undefined;
  const msg = { ...parsed, status: parsed.status ?? activity };
  const generatedId = useId();
  const [expanded, setExpanded] = useState(true);
  const status = msg.status?.pending ? "pending" : msg.status?.ok ? "success"
    : msg.status ? "error" : (activityStatus === "running" ? "pending" : activityStatus ?? "unresolved");
  return <InboundActivityCard
    testId="chat-sent-to-card"
    headerTestId="chat-sent-header"
    statusTestId="chat-sent-status"
    statusLabel={msg.status?.label}
    message={{ id: eventId ?? generatedId, type: "user", content: msg.body }}
    title={`Sent to ${msg.recipient}${msg.mesh ? ` @ ${msg.mesh}` : ""}`}
    summary=""
    status={status}
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
