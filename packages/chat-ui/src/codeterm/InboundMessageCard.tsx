import type { NormalizedChatMessage as ChatMessage } from "@codeterm/chat-core";
import { MessageSquareReply, Network } from "lucide-react";
import type { OpenPlan, ResolvedChatPlanRef } from "./LinkedText";
import { CompactMessageContent } from "./CompactMessageContent";
import { InboundActivityCard } from "./InboundActivityCard";
import { decodeCliMessageText } from "./messageText";

interface Props {
  message: ChatMessage;
  sender: string;
  model?: string;
  channel?: "pane" | "mesh";
  expanded: boolean;
  onToggle: () => void;
  onFocusTab?: () => void;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: "plan" | "task" | "section") => ResolvedChatPlanRef | null;
  fontScale?: number;
}

export function InboundMessageCard({ message, sender, model, channel = "pane", expanded, onToggle, onFocusTab, onOpenPlan, resolvePlanRef, fontScale }: Props) {
  const isMesh = channel === "mesh";
  const summary = [isMesh ? "mesh" : "tab", model].filter(Boolean).join(" · ");
  return (
    <InboundActivityCard
      message={message}
      testId="chat-inbound-message"
      title={`from ${sender}`}
      summary={summary}
      chips={[isMesh ? "mesh" : "tab"]}
      visual={{ glyph: isMesh ? Network : MessageSquareReply, color: isMesh ? "var(--ct-cyan)" : "var(--ct-green)", label: "inbound" }}
      expanded={expanded}
      onToggle={onToggle}
      onFocusTab={onFocusTab}
    >
      <CompactMessageContent text={decodeCliMessageText(message.content)} fontScale={fontScale} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} />
    </InboundActivityCard>
  );
}
