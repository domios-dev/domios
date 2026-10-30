import React from "react";
import { localArtifactTextParts } from "@codeterm/chat-core";
import { RichMarkdown } from "../MarkdownBody";
import { ClipboardList, FileText, ListChecks } from "lucide-react";
import { extractPlanRefs, linkifyPlanRefs, type PlanRefLinkSegment } from "./planRefs";
import { canonicalMentions, type CanonicalMention } from "./canonicalMentions";
import { useChatBlockHost } from "../blocks/ChatBlockHost";
import { useLiveBinding } from "../blocks/liveBinding";
import { ReferenceChip } from "./ReferenceChip";
import { splitAttachedPunctuation } from "./attachedPunctuation";

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

export function LinkedText(props: Props) {
  const parts = localArtifactTextParts(props.text);
  return <>{parts.map((part, index) => part.directive
    ? <RichMarkdown key={index} text={part.text} />
    : <MentionLinkedText key={index} {...props} text={part.text} />)}</>;
}

function MentionLinkedText(props: Props) {
  const host = useChatBlockHost();
  // Without a mention host, task ids keep the plan-reference path that resolvePlanRef serves.
  const mentions = canonicalMentions(props.text).filter((mention) => mention.kind !== "task" || host.mentions);
  if (!mentions.length) return <PlainLinkedText {...props} />;
  const nodes: React.ReactNode[] = [];
  let cursor = 0;
  mentions.forEach((mention, index) => {
    nodes.push(<PlainLinkedText key={`text:${cursor}`} {...props} text={props.text.slice(cursor, mention.start)} />);
    const [attached] = splitAttachedPunctuation(props.text.slice(mention.end, mentions[index + 1]?.start));
    const chip = <MentionChip key={`mention:${mention.start}`} mention={mention} />;
    nodes.push(attached ? <NoBreakRun key={`mention:${mention.start}`}>{chip}{attached}</NoBreakRun> : chip);
    cursor = mention.end + attached.length;
  });
  nodes.push(<PlainLinkedText key={`text:${cursor}`} {...props} text={props.text.slice(cursor)} />);
  return <span style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{nodes}</span>;
}

function NoBreakRun({ children }: { children: React.ReactNode }) {
  return <span style={{ whiteSpace: "nowrap" }}>{children}</span>;
}

export function MentionChip({ mention }: { mention: CanonicalMention }) {
  const host = useChatBlockHost();
  const key = JSON.stringify([host.mentionTabId ?? "", mention.kind, mention.id]);
  const value = useLiveBinding(host.mentions?.binding, key, null);
  if (!value && mention.kind === "task") return <>{mention.text}</>;
  return <ReferenceChip kind={mention.kind} label={value?.title ?? mention.id} fullRef={mention.id}
    planId={value?.planId} targetId={value?.targetId} disabled={!value || value.state === "loading"} state={value?.state ?? (value ? "resolved" : "missing")}
    onOpen={() => host.mentions?.open(key)} />;
}

function PlainLinkedText({ text, onOpenPlan, className, resolvePlanRef }: Props) {
  const segments = linkifyPlanRefs(text);
  return (
    <span className={className} style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>
      {segments.map((seg, i) => {
        if (seg.kind === "text") return <React.Fragment key={i}>{segments[i - 1] ? splitAttachedPunctuation(seg.value)[1] : seg.value}</React.Fragment>;
        const next = segments[i + 1];
        const trailing = next?.kind === "text" ? splitAttachedPunctuation(next.value)[0] : "";
        const ref = <PlanReference segment={seg} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} />;
        return trailing ? <NoBreakRun key={i}>{ref}{trailing}</NoBreakRun> : <React.Fragment key={i}>{ref}</React.Fragment>;
      })}
    </span>
  );
}

export function PlanReference({ segment: seg, onOpenPlan, resolvePlanRef }: Pick<Props, "onOpenPlan" | "resolvePlanRef"> & { segment: PlanRefLinkSegment }) {
  if (!onOpenPlan) return <>{seg.value}</>;
  const resolved = resolvePlanRef?.(seg.planId, seg.targetId, seg.targetKind) ?? null;
  const label = compactLabel(seg, resolved);
  const status = stateSuffix(resolved?.state);
  const disabled = resolved != null && resolved.state !== "resolved" && resolved.state !== "archived" && resolved.state !== "unowned";
  const fullRef = resolved?.fullRef ?? fullRefLabel(seg);
  return <ReferenceChip kind={seg.targetKind} label={label} fullRef={fullRef}
    status={status} disabled={disabled} state={resolved?.state ?? "loading"}
    planId={seg.planId} targetId={seg.targetId} onOpen={() => openRef(onOpenPlan, seg)} />;
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
