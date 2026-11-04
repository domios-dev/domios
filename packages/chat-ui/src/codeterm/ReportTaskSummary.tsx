import { FloatingTooltip } from "../FloatingTooltip";
import { useChatBlockHost } from "../blocks/ChatBlockHost";
import { useLiveBinding } from "../blocks/liveBinding";
import type { ResolvedChatPlanRef } from "./LinkedText";
import { splitTaskLabel, truncateChipTitle } from "./ReferenceChip";

export function ReportTaskSummary({ task, resolvePlanRef }: {
  task?: string | null;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: "plan" | "task" | "section") => ResolvedChatPlanRef | null;
}) {
  const host = useChatBlockHost();
  const id = task?.match(/\btask_[a-z0-9]+\b/)?.[0];
  const mention = useLiveBinding(id ? host.mentions?.binding : undefined, JSON.stringify([host.mentionTabId ?? "", "task", id ?? ""]), null);
  if (!task) return <>status update</>;
  const planId = task.match(/\bpl_[a-z0-9]+\b/)?.[0];
  const resolved = planId && id ? resolvePlanRef?.(planId, id, "task") : null;
  const full = mention?.title ?? resolved?.title ?? task;
  const label = splitTaskLabel(full);
  return <FloatingTooltip content={full} tooltipStyle={{ width: "max-content", maxWidth: "min(22rem, calc(100vw - 24px))" }} focusable={false} className="inline-flex min-w-0 max-w-full whitespace-nowrap">
    <span className="truncate">{label.key}{label.title ? ` ${truncateChipTitle(label.title)}` : ""}</span>
  </FloatingTooltip>;
}
