import type { CodetermInvocation } from "./recognize";
import { parseSend, type TabNameResolver } from "./templates";
import type { OpenPlan, ResolvedChatPlanRef } from "./LinkedText";
import { pickUiFontScaleValue } from "../uiFontScale";
import { CrossTabMessageShell } from "../crossTabBubble";
import { CompactMessageContent } from "./CompactMessageContent";

interface Props {
  inv: CodetermInvocation;
  result?: string;
  resolveTabName?: TabNameResolver;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: "plan" | "task" | "section") => ResolvedChatPlanRef | null;
  fontScale?: number;
}

export function SentMessageBubble({ inv, result, resolveTabName, onOpenPlan, resolvePlanRef, fontScale }: Props) {
  const msg = parseSend(inv, result, resolveTabName);
  const bodyText = pickUiFontScaleValue(fontScale, ["text-xs", "text-sm", "text-base", "text-lg"]);

  const header = (
    <div
      data-testid="chat-sent-header"
      className="mb-1 flex items-center gap-1.5 text-[11.5px] font-medium"
      style={{ color: "color-mix(in srgb, var(--ct-accent) 72%, var(--ct-muted-fg))" }}
    >
      <span className="flex-shrink-0" style={{ opacity: 0.85 }}>
        {msg.mesh ? "🛰" : "→"}
      </span>
      <span className="truncate">
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
        text={msg.formattedAvailable ? msg.body : inv.raw}
        onOpenPlan={onOpenPlan}
        resolvePlanRef={resolvePlanRef}
        fontScale={fontScale}
        expandTestId="chat-sent-expand"
      />
    </CrossTabMessageShell>
  );
}
