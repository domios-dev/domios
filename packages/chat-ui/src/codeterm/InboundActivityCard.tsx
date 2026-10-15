import type { NormalizedChatMessage as ChatMessage, ToolEvent } from "@codeterm/chat-core";
import type { ReactNode } from "react";
import { Copy, MessageSquarePlus } from "lucide-react";
import { useChatBlockHost, type ChatCardMenuItem } from "../blocks/ChatBlockHost";
import { askExcerpt, copyPlainText } from "../ChatCardMenu";
import { ToolCardActionButton, ToolCardChips, ToolCardFrame } from "../toolcard/ToolCardFrame";
import { decodeCliMessageText } from "./messageText";
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
  const host = useChatBlockHost();
  const event = inboundActivityEvent(message, summary, status);
  const text = decodeCliMessageText(message.content).trim();
  const menu: ChatCardMenuItem[] = [];
  if (text) menu.push({ label: "Copy text", icon: <Copy size={14} aria-hidden />, onSelect: () => copyPlainText(text) });
  if (host.askAbout && text) {
    const askAbout = host.askAbout;
    menu.push({ label: "Ask about this", icon: <MessageSquarePlus size={14} aria-hidden />, onSelect: () => askAbout({ title: `${title}: ${summary}`, text: `About this ${visual.label} (${title}, ${summary}):\n\n${askExcerpt(text)}` }) });
  }

  return (
    <section role="group" aria-label={title}>
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
      metric={chips.length ? <ToolCardChips chips={chips} /> : undefined}
      menu={menu}
      action={onFocusTab ? <ToolCardActionButton label="Show Tab" onClick={onFocusTab} testId="inbound-show-tab" /> : undefined}
    >
      {children}
    </ToolCardFrame>
    </section>
  );
}
