import type { NormalizedChatMessage as ChatMessage } from "@codeterm/chat-core";
import { Clipboard, RadioTower } from "lucide-react";
import { useState } from "react";
import { InboundActivityCard } from "./InboundActivityCard";
import type { OpenPlan, ResolvedChatPlanRef } from "./LinkedText";
import { LinkedText } from "./LinkedText";
import type { CoordinationReportDescriptor } from "./coordinationReport";
import { formatCommunicationEndpoint } from "./templates";

interface Props {
  descriptor: CoordinationReportDescriptor;
  message: ChatMessage;
  sender: string;
  expanded: boolean;
  onToggle: () => void;
  onFocusTab?: () => void;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: "plan" | "task" | "section") => ResolvedChatPlanRef | null;
}

export function CoordinationReportCard({ descriptor, message, sender, expanded, onToggle, onFocusTab, onOpenPlan, resolvePlanRef }: Props) {
  const [copied, setCopied] = useState(false);
  const failed = /failed|overdue|incident/i.test(descriptor.event);
  const task = descriptor.task ?? "Unassigned task";
  const worker = sender.length <= 80 ? sender : `${sender.slice(0, 79)}…`;

  const copyDiagnostics = async () => {
    await navigator.clipboard.writeText(descriptor.diagnostics);
    setCopied(true);
  };

  return (
    <InboundActivityCard
      testId="chat-coordination-report"
      message={message}
      status={failed ? "error" : "success"}
      expanded={expanded}
      onToggle={onToggle}
      title={`Report from ${formatCommunicationEndpoint(worker)}`}
      summary={`${descriptor.event} · ${task}`}
      presentationSummary={<><span>{descriptor.event} · </span><LinkedText text={task} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} /></>}
      visual={{ glyph: RadioTower, color: failed ? "var(--ct-red)" : "var(--ct-accent)", label: "coordination" }}
      chips={[descriptor.state]}
      onFocusTab={onFocusTab}
      disclosureLabel="coordination Details"
    >
      <div data-testid="chat-coordination-details" className="grid gap-2">
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs font-semibold" style={{ color: "var(--ct-fg)" }}>Details</span>
          <button
            type="button"
            onClick={() => void copyDiagnostics()}
            aria-label="Copy coordination diagnostics"
            className="inline-flex items-center gap-1 rounded border px-2 py-0.5 text-[11px]"
            style={{ borderColor: "var(--ct-border)", color: "var(--ct-muted-fg)" }}
          >
            <Clipboard aria-hidden="true" size={12} />
            {copied ? "Copied" : "Copy"}
          </button>
        </div>
        <pre className="m-0 max-h-64 overflow-auto whitespace-pre-wrap break-words font-mono text-xs" style={{ color: "var(--ct-fg)" }}>
          {descriptor.diagnostics}
        </pre>
      </div>
    </InboundActivityCard>
  );
}
