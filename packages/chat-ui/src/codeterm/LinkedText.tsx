import React from "react";
import { ClipboardList, FileText, ListChecks } from "lucide-react";
import { extractPlanRefs, linkifyPlanRefs, type PlanRefLinkSegment } from "./planRefs";

export type PlanRefKind = PlanRefLinkSegment["targetKind"];
export type OpenPlan = (planId: string, focusId?: string, focusKind?: PlanRefKind) => void;

interface Props {
  text: string;
  onOpenPlan?: OpenPlan;
  className?: string;
}

const REF_LABEL: Record<PlanRefKind, string> = {
  plan: "Plan",
  task: "Task",
  section: "Section",
};

function RefIcon({ kind, size = 12 }: { kind: PlanRefKind; size?: number }) {
  const Icon = kind === "task" ? ListChecks : kind === "section" ? FileText : ClipboardList;
  return <Icon size={size} strokeWidth={2} className="shrink-0" />;
}

function openRef(onOpenPlan: OpenPlan | undefined, seg: PlanRefLinkSegment) {
  onOpenPlan?.(seg.planId, seg.targetId, seg.targetKind);
}

/** Render plain text with CodeTerm plan/task/section mentions as clickable deep-links. */
export function LinkedText({ text, onOpenPlan, className }: Props) {
  const segments = linkifyPlanRefs(text);
  return (
    <span className={className} style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>
      {segments.map((seg, i) => {
        if (seg.kind === "text") return <React.Fragment key={i}>{seg.value}</React.Fragment>;
        if (!onOpenPlan) return <React.Fragment key={i}>{seg.value}</React.Fragment>;
        return (
          <button
            key={i}
            type="button"
            data-testid={`chat-${seg.targetKind}-link`}
            data-ref-kind={seg.targetKind}
            data-target-id={seg.targetId}
            data-plan-id={seg.planId}
            onClick={(e) => {
              e.stopPropagation();
              openRef(onOpenPlan, seg);
            }}
            className="inline-flex items-center gap-1 rounded px-1 py-0.5 align-baseline font-mono text-[0.92em] transition-colors hover:brightness-110"
            style={{ color: "var(--ct-accent, #6ea8fe)" }}
          >
            <RefIcon kind={seg.targetKind} />
            {seg.value}
          </button>
        );
      })}
    </span>
  );
}

export function PlanRefCards({ text, onOpenPlan }: { text: string; onOpenPlan?: OpenPlan }) {
  const refs = extractPlanRefs(text);
  if (refs.length === 0) return null;
  return (
    <div className="mt-2 flex flex-wrap gap-1.5" data-testid="chat-plan-ref-cards">
      {refs.map((ref) => (
        <button
          key={`${ref.targetKind}:${ref.planId}:${ref.targetId ?? ""}`}
          type="button"
          data-testid="chat-plan-ref-card"
          data-ref-kind={ref.targetKind}
          data-target-id={ref.targetId}
          data-plan-id={ref.planId}
          disabled={!onOpenPlan}
          onClick={(e) => {
            e.stopPropagation();
            openRef(onOpenPlan, ref);
          }}
          className="inline-flex min-w-0 max-w-full items-center gap-1.5 rounded-md border px-2 py-1 text-xs transition-colors hover:brightness-110 disabled:cursor-default"
          style={{
            background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 10%, transparent)",
            borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 30%, transparent)",
            color: "var(--ct-fg)",
          }}
        >
          <RefIcon kind={ref.targetKind} size={13} />
          <span className="shrink-0 font-medium" style={{ color: "var(--ct-accent, #58a6ff)" }}>
            {REF_LABEL[ref.targetKind]}
          </span>
          <span className="min-w-0 truncate font-mono" style={{ color: "var(--ct-muted-fg)" }}>
            {ref.value}
          </span>
        </button>
      ))}
    </div>
  );
}
