import type { CodetermInvocation } from "./recognize";
import { parseSend, type CardStatus, type PeerNameResolver, type TabNameResolver } from "./templates";
import type { OpenPlan, ResolvedChatPlanRef } from "./LinkedText";
import { pickUiFontScaleValue } from "../uiFontScale";
import { CrossTabMessageShell } from "../crossTabBubble";
import { CompactMessageContent } from "./CompactMessageContent";
import { RadioTower, Send } from "lucide-react";

interface Props {
  inv: CodetermInvocation;
  result?: string;
  resolveTabName?: TabNameResolver;
  resolvePeerName?: PeerNameResolver;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: "plan" | "task" | "section") => ResolvedChatPlanRef | null;
  fontScale?: number;
  technicalCommand?: string;
  activityStatus?: "pending" | "success" | "error" | "unresolved" | "interrupted" | "running";
}

export function SentMessageBubble({ inv, result, resolveTabName, resolvePeerName, onOpenPlan, resolvePlanRef, fontScale, technicalCommand, activityStatus }: Props) {
  const parsed = parseSend(inv, result, resolveTabName, resolvePeerName);
  const activity: CardStatus | undefined = activityStatus === "pending" || activityStatus === "running"
    ? { ok: false, label: "pending", pending: true }
    : activityStatus === "success" ? { ok: true, label: "sent" }
      : activityStatus === "error" ? { ok: false, label: "failed" }
        : activityStatus === "unresolved" || activityStatus === "interrupted" ? { ok: false, label: "unconfirmed", pending: true }
          : undefined;
  const msg = { ...parsed, status: parsed.status ?? activity };
  const bodyText = pickUiFontScaleValue(fontScale, ["text-xs", "text-sm", "text-base", "text-lg"]);

  const header = (
    <div
      data-testid="chat-sent-header"
      className="mb-1 flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-[11.5px] font-medium"
      style={{ color: "color-mix(in srgb, var(--ct-accent) 72%, var(--ct-muted-fg))" }}
    >
      <span className="flex-shrink-0" style={{ opacity: 0.85 }}>
        {msg.mesh ? <RadioTower aria-hidden="true" size={13} /> : <Send aria-hidden="true" size={13} />}
      </span>
      <span className="flex-shrink-0">Sent to</span>
      <span className="min-w-0 flex-1 truncate">
        {msg.recipient}
        {msg.mesh ? ` @ ${msg.mesh}` : ""}
      </span>
      {msg.status && (
        <span
          data-testid="chat-sent-status"
          role="status"
          className="flex-shrink-0"
          title={msg.status.label}
          aria-label={msg.status.label}
          style={{
            color: msg.status.pending
              ? "var(--ct-yellow)"
              : msg.status.ok
                ? "var(--ct-ok, #41d18a)"
                : "var(--ct-err, #ff6b6b)",
          }}
        >
          {msg.status.pending ? "…" : msg.status.ok ? "✓" : "✗"}
        </span>
      )}
    </div>
  );

  return (
    <CrossTabMessageShell
      header={header}
      textSize={bodyText}
      bubbleTestId="chat-sent-message"
    >
      <CompactMessageContent
        text={msg.body}
        onOpenPlan={onOpenPlan}
        resolvePlanRef={resolvePlanRef}
        fontScale={fontScale}
        expandTestId="chat-sent-expand"
      />
      {msg.status?.label === "failed" && msg.status.detail && <p role="alert" className="mt-1 text-xs" style={{ color: "var(--ct-err, #ff6b6b)" }}>{msg.status.detail.slice(0, 500)}</p>}
      {msg.truncated && <p role="note" className="mt-1 text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>Message preview capped at 16 KB.</p>}
      {technicalCommand && <details data-testid="raw-command-disclosure" className="mt-2 min-w-0 text-[11px]">
        <summary className="cursor-pointer" style={{ color: "var(--ct-muted-fg)" }}>Technical details</summary>
        <pre className="m-0 mt-1 whitespace-pre-wrap break-all" style={{ color: "var(--ct-muted-fg)" }}>{technicalCommand}</pre>
      </details>}
    </CrossTabMessageShell>
  );
}
