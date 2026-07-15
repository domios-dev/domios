import type { NormalizedChatMessage as ChatMessage } from "@codeterm/chat-core";
import { MessageSquareText } from "lucide-react";
import { InboundActivityCard } from "./InboundActivityCard";
import type { OpenPlan, ResolvedChatPlanRef } from "./LinkedText";
import { LinkedText } from "./LinkedText";
import { hasPlanRefs } from "./planRefs";

interface ReportField {
  label: string;
  value: string;
}

export interface InboundReportDescriptor {
  task?: string;
  status?: string;
  sha?: string;
  fields: ReportField[];
}

const FIELD_RE = /^(task|status|branch|sha|tests|changed|skipped|changes|tests run|gates requested|followup):\s*(.*)$/i;

export function parseInboundReport(text: string): InboundReportDescriptor {
  const fields: ReportField[] = [];
  for (const part of text.split(/\r?\n|\s+\|\s+(?=[A-Z][A-Z ]+:)/)) {
    const match = FIELD_RE.exec(part.trim());
    if (!match) continue;
    fields.push({ label: match[1].toLowerCase().replace(" run", ""), value: match[2].trim() });
  }
  const value = (label: string) => {
    for (let index = fields.length - 1; index >= 0; index -= 1) {
      if (fields[index].label === label) return fields[index].value;
    }
    return undefined;
  };
  return { task: value("task"), status: value("status"), sha: value("sha"), fields };
}

interface Props {
  message: ChatMessage;
  sender: string;
  expanded: boolean;
  onToggle: () => void;
  onFocusTab?: () => void;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: "plan" | "task" | "section") => ResolvedChatPlanRef | null;
}

function taskObject(task: string | undefined): string {
  if (!task) return "status update";
  const parts = task.split("/");
  return parts[parts.length - 1] || task;
}

export function InboundReportCard({ message, sender, expanded, onToggle, onFocusTab, onOpenPlan, resolvePlanRef }: Props) {
  const descriptor = parseInboundReport(message.content);
  const failed = /^(?:blocked|error|failed)$/i.test(descriptor.status ?? "");
  const chips = [descriptor.status, descriptor.sha].filter((value): value is string => Boolean(value));

  return (
    <InboundActivityCard
      testId="chat-inbound-report"
      message={message}
      status={failed ? "error" : "success"}
      expanded={expanded}
      onToggle={onToggle}
      title={`report · ${sender}`}
      summary={taskObject(descriptor.task)}
      visual={{ glyph: MessageSquareText, color: "var(--ct-magenta)", label: "report" }}
      chips={chips}
      onFocusTab={onFocusTab}
    >
      <dl className="grid max-h-64 grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-1 overflow-y-auto text-xs">
        {descriptor.fields.map((field, index) => (
          <div key={`${field.label}-${index}`} className="contents">
            <dt className="font-mono" style={{ color: "var(--ct-muted-fg)" }}>{field.label}</dt>
            <dd className="min-w-0 break-words" style={{ color: "var(--ct-fg)" }}>
              {hasPlanRefs(field.value) ? <LinkedText text={field.value} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} /> : field.value}
            </dd>
          </div>
        ))}
      </dl>
    </InboundActivityCard>
  );
}
