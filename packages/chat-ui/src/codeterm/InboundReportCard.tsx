import type { NormalizedChatMessage as ChatMessage } from "@codeterm/chat-core";
import { MessageSquareText } from "lucide-react";
import { InboundActivityCard } from "./InboundActivityCard";
import type { OpenPlan, ResolvedChatPlanRef } from "./LinkedText";
import { ReportTaskSummary } from "./ReportTaskSummary";
import { decodeCliMessageText } from "./messageText";
import { pickUiFontScaleValue } from "../uiFontScale";
import { CoordinationReportCard } from "./CoordinationReportCard";
import { parseCoordinationReport } from "./coordinationReport";
import { formatCommunicationEndpoint } from "./templates";
import { CompactMessageContent } from "./CompactMessageContent";
import {
  parseDeliveredTaskOutcome,
  reportContentBody,
  reportOutcomeLine,
  reportOutcomeTone,
  reportVerdict,
  shortSha,
  type ReportTaskOutcome,
} from "./reportOutcome";

export interface InboundReportDescriptor {
  task?: string;
  status?: string;
  sha?: string;
  /** The free-form message below the structured block, verbatim. */
  body: string;
}

// Only host envelope metadata belongs to the card header; the report body is Markdown.
export function parseInboundReport(text: string): InboundReportDescriptor {
  const lines = decodeCliMessageText(text).split("\n");
  const metadata: Record<string, string> = {};
  let cut = 0;
  for (; cut < lines.length; cut += 1) {
    const match = /^(task|status|branch|sha):\s*(.*)$/.exec(lines[cut]);
    if (!match) break;
    metadata[match[1]] = match[2].trim();
  }
  return { task: metadata.task, status: metadata.status, sha: metadata.sha,
    body: lines.slice(cut).join("\n").trim() };
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
  const tone = reportOutcomeTone(outcome);
  const failed = tone === "error" || (tone === null && /^(?:blocked|error|failed)$/i.test(descriptor.status ?? ""));
  const chips = [reportVerdict(descriptor.status, outcome), shortSha(descriptor.sha)]
    .filter((value): value is string => Boolean(value));
  const textSize = pickUiFontScaleValue(fontScale, ["text-xs", "text-sm", "text-base", "text-lg"]);
  const body = outcome?.reportOutcome === "delivered_unattached" ? "" : reportContentBody(descriptor.body);
  const diagnostics = outcome?.reportOutcome === "delivered_unattached"
    ? descriptor.body
    : outcome?.reportOutcome && body !== descriptor.body
      ? descriptor.body
      : null;
  const outcomeLine = reportOutcomeLine(outcome);

  return (
    <InboundActivityCard
      testId="chat-inbound-report"
      message={message}
      status={failed ? "error" : "success"}
      expanded={expanded}
      onToggle={onToggle}
      title={`Report from ${formatCommunicationEndpoint(sender)}`}
      summary={taskObject(descriptor.task)}
      presentationSummary={<ReportTaskSummary task={descriptor.task} resolvePlanRef={resolvePlanRef} />}
      visual={{ glyph: MessageSquareText, color: "var(--ct-magenta)", label: "report" }}
      chips={chips}
      onFocusTab={onFocusTab}
      disclosureLabel="report Details"
    >
      <div className={`ct-compact-scroll grid gap-2.5 leading-relaxed ${textSize}`}>
        {outcomeLine && (
          <div
            data-testid={tone === "error" ? "chat-inbound-report-incident" : "chat-inbound-report-outcome"}
            className="min-w-0 break-words"
            style={{ color: tone === "error" ? "var(--ct-red)" : "var(--ct-muted-fg)" }}
          >
            {outcomeLine}
          </div>
        )}
        {body && (
          <div data-testid="chat-inbound-report-body" className="min-w-0 break-words" style={{ color: "var(--ct-fg)" }}>
            <CompactMessageContent markdown text={body} fontScale={fontScale} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} collapseAfter={Number.POSITIVE_INFINITY} />
          </div>
        )}
        {diagnostics && (
          <pre
            data-testid="chat-inbound-report-diagnostics"
            className="m-0 whitespace-pre-wrap break-words font-mono text-[11px]"
            style={{ color: "var(--ct-muted-fg)" }}
          >
            {diagnostics}
          </pre>
        )}
      </div>
    </InboundActivityCard>
  );
}
