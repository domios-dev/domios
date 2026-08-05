import { useState } from "react";
import { Send } from "lucide-react";
import { InboundActivityCard } from "./InboundActivityCard";
import type { OpenPlan, ResolvedChatPlanRef } from "./LinkedText";
import { LinkedText } from "./LinkedText";
import { hasPlanRefs } from "./planRefs";
import { decodeCliMessageText } from "./messageText";
import { pickUiFontScaleValue } from "../uiFontScale";
import { formatCommunicationEndpoint } from "./templates";
import {
  reportContentBody,
  reportIncidentLine,
  reportVerdict,
  shortSha,
  type ReportTaskOutcome,
} from "./reportOutcome";

/** One accepted report as its sender sees it. Every field is truth the daemon
 *  already decided for this `reportId`; nothing here is read off a screen. */
export interface OutboundReportDescriptor {
  reportId: string;
  recipient: string;
  task: string | null;
  status: string | null;
  sha: string | null;
  message: string;
  taskOutcome: ReportTaskOutcome | null;
}

interface Props {
  report: OutboundReportDescriptor;
  onFocusTab?: () => void;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: "plan" | "task" | "section") => ResolvedChatPlanRef | null;
  fontScale?: number;
  defaultExpanded?: boolean;
}

export function OutboundReportCard({ report, onFocusTab, onOpenPlan, resolvePlanRef, fontScale, defaultExpanded = false }: Props) {
  const [expanded, setExpanded] = useState(defaultExpanded);
  const outcome = report.taskOutcome;
  const chips = [reportVerdict(report.status, outcome), shortSha(report.sha)]
    .filter((value): value is string => Boolean(value));
  const textSize = pickUiFontScaleValue(fontScale, ["text-xs", "text-sm", "text-base", "text-lg"]);
  const body = reportContentBody(decodeCliMessageText(report.message)).trim();
  const incident = reportIncidentLine(outcome);
  const failed = /^(?:blocked|error|failed)$/i.test(report.status ?? "");

  return (
    <InboundActivityCard
      testId="chat-outbound-report"
      message={{ id: `report-${report.reportId}`, type: "user", content: body }}
      status={failed ? "error" : "success"}
      expanded={expanded}
      onToggle={() => setExpanded((previous) => !previous)}
      title={`Report to ${formatCommunicationEndpoint(report.recipient)}`}
      summary={report.task ?? "status update"}
      visual={{ glyph: Send, color: "var(--ct-magenta)", label: "report" }}
      chips={chips}
      onFocusTab={onFocusTab}
      disclosureLabel="report Details"
    >
      <div className={`grid max-h-64 gap-2 overflow-y-auto ${textSize}`}>
        {incident && (
          <div
            data-testid="chat-outbound-report-incident"
            className="min-w-0 break-words"
            style={{ color: "var(--ct-red)" }}
          >
            {incident}
          </div>
        )}
        {body && (
          <div
            data-testid="chat-outbound-report-body"
            className="min-w-0 whitespace-pre-wrap break-words"
            style={{ color: "var(--ct-fg)" }}
          >
            {hasPlanRefs(body) ? <LinkedText text={body} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} /> : body}
          </div>
        )}
      </div>
    </InboundActivityCard>
  );
}
