import type { NormalizedChatMessage as ChatMessage } from "@codeterm/chat-core";
import { MessageSquareText } from "lucide-react";
import { InboundActivityCard } from "./InboundActivityCard";
import type { OpenPlan, ResolvedChatPlanRef } from "./LinkedText";
import { LinkedText } from "./LinkedText";
import { hasPlanRefs } from "./planRefs";
import { decodeCliMessageText } from "./messageText";
import { pickUiFontScaleValue } from "../uiFontScale";
import { CoordinationReportCard } from "./CoordinationReportCard";
import { parseCoordinationReport } from "./coordinationReport";
import { formatCommunicationEndpoint } from "./templates";
import {
  parseDeliveredTaskOutcome,
  reportContentBody,
  reportIncidentLine,
  reportVerdict,
  shortSha,
  type ReportTaskOutcome,
} from "./reportOutcome";

/** Identity the dense header row already states; repeating it as body rows is
 *  what made the card four lines of metadata before its first sentence. */
const HEADER_FIELDS = new Set(["task", "status", "branch", "sha"]);

interface ReportField {
  label: string;
  value: string;
}

export interface InboundReportDescriptor {
  task?: string;
  status?: string;
  sha?: string;
  fields: ReportField[];
  /** The free-form message below the structured block, verbatim. */
  body: string;
}

const FIELD_RE = /^(task|status|branch|sha|tests|changed|skipped|changes|tests run|gates requested|followup):\s*(.*)$/i;
const COMPOUND_SPLIT_RE = /\s+\|\s+(?=[A-Z][A-Z ]+:)/;

// AgentReport::inject_body emits structured fields as a leading block, then the
// message: the first non-field line opens the body, so quoted prose stays body.
export function parseInboundReport(text: string): InboundReportDescriptor {
  const lines = decodeCliMessageText(text).split("\n");
  const fields: ReportField[] = [];
  let cut = 0;
  for (; cut < lines.length; cut += 1) {
    const matches = lines[cut].split(COMPOUND_SPLIT_RE).map((segment) => FIELD_RE.exec(segment.trim()));
    if (matches.some((match) => !match)) break;
    for (const match of matches) {
      fields.push({ label: match![1].toLowerCase().replace(" run", ""), value: match![2].trim() });
    }
  }
  const value = (label: string) => {
    for (let index = fields.length - 1; index >= 0; index -= 1) {
      if (fields[index].label === label) return fields[index].value;
    }
    return undefined;
  };
  return {
    task: value("task"),
    status: value("status"),
    sha: value("sha"),
    fields,
    body: lines.slice(cut).join("\n").trim(),
  };
}

interface Props {
  message: ChatMessage;
  sender: string;
  expanded: boolean;
  onToggle: () => void;
  onFocusTab?: () => void;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: "plan" | "task" | "section") => ResolvedChatPlanRef | null;
  fontScale?: number;
  /** Structured truth when the report event is linked; the delivered incident
   *  header is the fallback so a refused transition is never silently dropped. */
  taskOutcome?: ReportTaskOutcome | null;
}

function taskObject(task: string | undefined): string {
  if (!task) return "status update";
  const parts = task.split("/");
  return parts[parts.length - 1] || task;
}

export function InboundReportCard({ message, sender, expanded, onToggle, onFocusTab, onOpenPlan, resolvePlanRef, fontScale, taskOutcome }: Props) {
  const coordination = parseCoordinationReport(message.content);
  if (coordination) {
    return (
      <CoordinationReportCard
        descriptor={coordination}
        message={message}
        sender={sender}
        expanded={expanded}
        onToggle={onToggle}
        onFocusTab={onFocusTab}
        onOpenPlan={onOpenPlan}
        resolvePlanRef={resolvePlanRef}
      />
    );
  }
  const descriptor = parseInboundReport(message.content);
  const outcome = taskOutcome ?? parseDeliveredTaskOutcome(descriptor.body);
  const failed = /^(?:blocked|error|failed)$/i.test(descriptor.status ?? "");
  const chips = [reportVerdict(descriptor.status, outcome), shortSha(descriptor.sha)]
    .filter((value): value is string => Boolean(value));
  const textSize = pickUiFontScaleValue(fontScale, ["text-xs", "text-sm", "text-base", "text-lg"]);
  const body = reportContentBody(descriptor.body);
  const incident = reportIncidentLine(outcome);
  const detailFields = descriptor.fields.filter((field) => !HEADER_FIELDS.has(field.label));

  return (
    <InboundActivityCard
      testId="chat-inbound-report"
      message={message}
      status={failed ? "error" : "success"}
      expanded={expanded}
      onToggle={onToggle}
      title={`Report from ${formatCommunicationEndpoint(sender)}`}
      summary={taskObject(descriptor.task)}
      visual={{ glyph: MessageSquareText, color: "var(--ct-magenta)", label: "report" }}
      chips={chips}
      onFocusTab={onFocusTab}
      disclosureLabel="report Details"
    >
      <div className={`ct-compact-scroll grid max-h-64 gap-2 overflow-y-auto ${textSize}`}>
        {incident && (
          <div
            data-testid="chat-inbound-report-incident"
            className="min-w-0 break-words"
            style={{ color: "var(--ct-red)" }}
          >
            {incident}
          </div>
        )}
        {body && (
          <div
            data-testid="chat-inbound-report-body"
            className="min-w-0 whitespace-pre-wrap break-words"
            style={{ color: "var(--ct-fg)" }}
          >
            {hasPlanRefs(body) ? <LinkedText text={body} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} /> : body}
          </div>
        )}
        {detailFields.length > 0 && (
          <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-1">
            {detailFields.map((field, index) => (
              <div key={`${field.label}-${index}`} className="contents">
                <dt className="font-mono" style={{ color: "var(--ct-muted-fg)" }}>{field.label}</dt>
                <dd className="min-w-0 break-words" style={{ color: "var(--ct-fg)" }}>
                  {hasPlanRefs(field.value) ? <LinkedText text={field.value} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} /> : field.value}
                </dd>
              </div>
            ))}
          </dl>
        )}
      </div>
    </InboundActivityCard>
  );
}
