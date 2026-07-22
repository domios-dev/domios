import React from "react";
import { ClipboardList, FileText, ListChecks } from "lucide-react";
import { extractPlanRefs, linkifyPlanRefs, type PlanRefLinkSegment } from "./planRefs";

export type PlanRefKind = PlanRefLinkSegment["targetKind"];
export type OpenPlan = (planId: string, focusId?: string, focusKind?: PlanRefKind) => void;
export interface ResolvedChatPlanRef {
  title: string;
  planTitle: string;
  taskTitle?: string;
  sectionTitle?: string;
  state: "resolved" | "archived" | "unowned" | "stale" | "missing" | "loading" | "unreachable" | "permission";
  rawLabel: string;
  fullRef: string;
}

interface Props {
  text: string;
  onOpenPlan?: OpenPlan;
  className?: string;
  resolvePlanRef?: (planId: string, targetId?: string, targetKind?: PlanRefKind) => ResolvedChatPlanRef | null;
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
function compactLabel(seg: PlanRefLinkSegment, resolved: ResolvedChatPlanRef | null): string {
  if (resolved?.title?.trim()) return resolved.title.trim();
  return resolved?.rawLabel ?? seg.value;
}

function fullRefLabel(seg: PlanRefLinkSegment): string {
  return seg.targetId ? `${seg.planId}/${seg.targetId}` : seg.planId;
}

function stateSuffix(state: ResolvedChatPlanRef["state"] | undefined): string {
  switch (state) {
    case "loading": return "Loading";
    case "archived": return "Archived";
    case "unowned": return "Workspace unavailable";
    case "stale": return "Stale reference";
    case "missing": return "Missing";
    case "unreachable": return "Unavailable";
    case "permission": return "Permission denied";
    default: return "";
  }
}

export function LinkedText({ text, onOpenPlan, className, resolvePlanRef }: Props) {
  const segments = linkifyPlanRefs(text);
  return (
    <span className={className} style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>
      {segments.map((seg, i) => {
        if (seg.kind === "text") return <React.Fragment key={i}>{seg.value}</React.Fragment>;
        if (!onOpenPlan) return <React.Fragment key={i}>{seg.value}</React.Fragment>;
        const resolved = resolvePlanRef?.(seg.planId, seg.targetId, seg.targetKind) ?? null;
        const label = compactLabel(seg, resolved);
        const status = stateSuffix(resolved?.state);
        const disabled = resolved != null && resolved.state !== "resolved" && resolved.state !== "archived" && resolved.state !== "unowned";
        const fullRef = resolved?.fullRef ?? fullRefLabel(seg);
        return (
          <button
            key={i}
            type="button"
            data-testid={`chat-${seg.targetKind}-link`}
            data-ref-kind={seg.targetKind}
            data-ref-state={resolved?.state ?? "loading"}
            data-target-id={seg.targetId}
            data-plan-id={seg.planId}
            disabled={disabled}
            aria-label={`${REF_LABEL[seg.targetKind]} ${label}${status ? ` (${status})` : ""}. ${fullRef}`}
            title={status ? `${label} (${status})\n${fullRef}` : `${label}\n${fullRef}`}
            onClick={(e) => {
              e.stopPropagation();
              if (disabled) return;
              openRef(onOpenPlan, seg);
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              void navigator.clipboard.writeText(fullRef);
            }}
            className="inline-flex items-center gap-1 rounded px-1 py-0.5 align-baseline font-mono text-[0.92em] transition-colors hover:brightness-110"
            style={{ color: disabled ? "var(--ct-muted-fg, #8b949e)" : "var(--ct-accent, #6ea8fe)" }}
          >
            <RefIcon kind={seg.targetKind} />
            <span className="truncate max-w-[18rem] sm:max-w-[24rem]">{label}</span>
            {status && <span className="text-[0.82em] opacity-70">({status})</span>}
          </button>
        );
      })}
    </span>
  );
}

export function PlanRefCards({
  text,
  onOpenPlan,
  resolvePlanRef,
}: {
  text: string;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, targetId?: string, targetKind?: PlanRefKind) => ResolvedChatPlanRef | null;
}) {
  const refs = extractPlanRefs(text);
  if (refs.length === 0) return null;
  return (
    <div className="mt-2 flex flex-wrap gap-1.5" data-testid="chat-plan-ref-cards">
      {refs.map((ref) => (
        (() => {
          const resolved = resolvePlanRef?.(ref.planId, ref.targetId, ref.targetKind) ?? null;
          const label = compactLabel(ref, resolved);
          const status = stateSuffix(resolved?.state);
          const disabled = !onOpenPlan || (resolved != null && resolved.state !== "resolved" && resolved.state !== "archived" && resolved.state !== "unowned");
          const fullRef = resolved?.fullRef ?? fullRefLabel(ref);
          return (
        <button
          key={`${ref.targetKind}:${ref.planId}:${ref.targetId ?? ""}`}
          type="button"
          data-testid={`chat-${ref.targetKind}-link`}
          data-ref-kind={ref.targetKind}
          data-target-id={ref.targetId}
          data-plan-id={ref.planId}
          data-ref-state={resolved?.state ?? "loading"}
          disabled={disabled}
          aria-label={`${REF_LABEL[ref.targetKind]} ${label}${status ? ` (${status})` : ""}. ${fullRef}`}
          title={status ? `${label} (${status})\n${fullRef}` : `${label}\n${fullRef}`}
          onClick={(e) => {
            e.stopPropagation();
            if (disabled) return;
            openRef(onOpenPlan, ref);
          }}
          onContextMenu={(e) => {
            e.preventDefault();
            void navigator.clipboard.writeText(fullRef);
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
            {label}
          </span>
          {status && <span className="shrink-0 opacity-70">{status}</span>}
        </button>
          );
        })()
      ))}
    </div>
  );
}
