import { MODEL_BADGE_STYLE } from "../Sidebar/SidebarModelBadge";
import { Sparkles, SquareTerminal, Zap } from "lucide-react";
import type { NodeKind } from "./nodeDecision";

export function NodeKindBadge({ kind }: { kind: NodeKind }) {
  const Icon = kind.ai ? Sparkles : kind.label === "Command" || kind.label === "Script" ? SquareTerminal : Zap;
  return <span title={kind.detail ?? kind.label} aria-label={`${kind.ai ? "AI" : "Deterministic"}: ${kind.label}`} style={{ fontSize: 10, color: kind.ai ? "var(--ct-accent)" : "var(--ct-muted-fg)" }} className={`${MODEL_BADGE_STYLE} inline-flex max-w-full items-center gap-1 tracking-normal ${kind.ai ? "bg-[color-mix(in_srgb,var(--ct-accent)_8%,transparent)]" : "text-[var(--ct-muted-fg)]"}`}><Icon size={11} className="shrink-0" aria-hidden="true" /><span className="break-words">{kind.label}</span></span>;
}
