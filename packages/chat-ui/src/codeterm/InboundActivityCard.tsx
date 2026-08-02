import type { NormalizedChatMessage as ChatMessage, ToolEvent } from "@codeterm/chat-core";
import type { ReactNode } from "react";
import { ToolCardActionButton, ToolCardFrame } from "../toolcard/ToolCardFrame";
import type { ToolKindVisual } from "../toolcard/toolKindVisual";

interface Props {
  message: ChatMessage;
  testId: string;
  title: string;
  summary: string;
  presentationSummary?: ReactNode;
  chips?: string[];
  status?: ToolEvent["status"];
  visual: ToolKindVisual;
  expanded: boolean;
  onToggle: () => void;
  onFocusTab?: () => void;
  children: ReactNode;
  disclosureLabel?: string;
}

export function inboundActivityEvent(message: ChatMessage, summary: string, status: ToolEvent["status"]): ToolEvent {
  return {
    id: message.id,
    phase: "result",
    status,
    kind: "generic",
    summary,
    message,
  };
}

export function InboundActivityCard({ message, testId, title, summary, presentationSummary, chips = [], status = "success", visual, expanded, onToggle, onFocusTab, children, disclosureLabel }: Props) {
  const event = inboundActivityEvent(message, summary, status);

  return (
    <ToolCardFrame
      testId={testId}
      event={event}
      expanded={expanded}
      onToggle={onToggle}
      title={title}
      summary={presentationSummary ?? summary}
      variant="codeterm"
      visualOverride={visual}
      disclosureLabel={disclosureLabel}
      metric={chips.length ? <span className="flex min-w-0 gap-1">{chips.map((chip) => <span key={chip} className="max-w-40 truncate rounded border px-1.5 py-0.5 text-[10px]" style={{ borderColor: "var(--ct-border)", color: "var(--ct-muted-fg)" }}>{chip}</span>)}</span> : undefined}
      action={onFocusTab ? <ToolCardActionButton label="Show Tab" onClick={onFocusTab} testId="inbound-show-tab" /> : undefined}
    >
      {children}
    </ToolCardFrame>
  );
}
