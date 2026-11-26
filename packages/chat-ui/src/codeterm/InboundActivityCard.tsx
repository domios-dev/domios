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
  technicalCommand?: string;
  showTechnicalDetails?: boolean;
  focusTabTestId?: string;
  headerTestId?: string;
  statusTestId?: string;
  statusLabel?: string;
  testId: string;
  title: string;
  summary: string;
  subtitle?: ReactNode;
  presentationSummary?: ReactNode;
  chips?: string[];
  status?: ToolEvent["status"];
  visual: ToolKindVisual;
  expanded: boolean;
  onToggle: () => void;
  onFocusTab?: () => void;
  focusTabName?: string;
  children: ReactNode;
  disclosureLabel?: string;
  disclosureToggleLabel?: { expanded: string; collapsed: string };
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

export function InboundActivityCard({ message, testId, title, summary, subtitle, presentationSummary, chips = [], status = "success", visual, expanded, onToggle, onFocusTab, focusTabName, children, disclosureLabel, disclosureToggleLabel, technicalCommand, showTechnicalDetails = true, focusTabTestId = "inbound-show-tab", headerTestId, statusTestId, statusLabel }: Props) {
  const host = useChatBlockHost();
  const event = { ...inboundActivityEvent(message, summary, status), detail: technicalCommand };
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
      communication
      showTechnicalDetails={showTechnicalDetails}
      headerTestId={headerTestId}
      statusTestId={statusTestId}
      statusLabel={statusLabel}
      testId={testId}
      event={event}
      expanded={expanded}
      onToggle={onToggle}
      title={title}
      subtitle={subtitle}
      summary={presentationSummary ?? summary}
      variant="codeterm"
      visualOverride={visual}
      disclosureLabel={disclosureLabel}
      disclosureToggleLabel={disclosureToggleLabel}
      metric={chips.length ? <ToolCardChips chips={chips} wrap /> : undefined}
      menu={menu}
      action={onFocusTab ? <ToolCardActionButton label="Show Tab" ariaLabel={focusTabName ? `Show tab ${focusTabName}` : undefined} compactIcon={Boolean(focusTabName)} onClick={onFocusTab} testId={focusTabTestId} /> : undefined}
    >
      {children}
    </ToolCardFrame>
    </section>
  );
}
