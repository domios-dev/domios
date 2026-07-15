import type { NormalizedChatMessage as ChatMessage, ToolEvent } from "@codeterm/chat-core";
import { MessageSquareText } from "lucide-react";
import { ToolCardFrame } from "../toolcard/ToolCardFrame";
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
  const event: ToolEvent = {
    id: message.id,
    phase: "result",
    status: failed ? "error" : "success",
    kind: "generic",
    summary: taskObject(descriptor.task),
    message,
  };
  const chips = [descriptor.status, descriptor.sha].filter((value): value is string => Boolean(value));

  return (
    <ToolCardFrame
      testId="chat-inbound-report"
      event={event}
      expanded={expanded}
      onToggle={onToggle}
      title={`report · ${sender}`}
      summary={taskObject(descriptor.task)}
      variant="codeterm"
      visualOverride={{ glyph: MessageSquareText, color: "var(--ct-magenta)", label: "report" }}
      metric={chips.length ? <span className="flex min-w-0 gap-1">{chips.map((chip) => <span key={chip} className="max-w-40 truncate rounded border px-1.5 py-0.5 text-[10px]" style={{ borderColor: "var(--ct-border)", color: "var(--ct-muted-fg)" }}>{chip}</span>)}</span> : undefined}
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
      {onFocusTab && (
        <button type="button" onClick={onFocusTab} className="mt-2 rounded-md border px-2.5 py-1 font-mono text-[11.5px]" style={{ borderColor: "color-mix(in srgb, var(--ct-accent) 32%, transparent)", color: "var(--ct-accent)", background: "color-mix(in srgb, var(--ct-accent) 12%, transparent)" }}>
          Focus Tab
        </button>
      )}
    </ToolCardFrame>
  );
}
