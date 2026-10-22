import React from "react";
import { ClipboardList, FileText, ListChecks, Terminal, Workflow } from "lucide-react";
export function ReferenceChip({ kind, label, fullRef, status, disabled, onOpen, planId, targetId, state }: {
  kind: "tab" | "blueprint" | "plan" | "task" | "section";
  label: string;
  fullRef: string;
  status?: string;
  disabled: boolean;
  onOpen?: () => void;
  planId?: string;
  targetId?: string;
  state?: string;
}): React.ReactElement {
  const Icon = kind === "tab" ? Terminal : kind === "blueprint" ? Workflow : kind === "task" ? ListChecks : kind === "section" ? FileText : ClipboardList;
  return <button type="button" data-testid={`chat-${kind}-link`} data-ref-kind={kind} data-ref-state={state ?? "resolved"}
    data-target-id={targetId} data-plan-id={planId} disabled={disabled}
    aria-label={`${kind[0].toUpperCase() + kind.slice(1)} ${label}${status ? ` (${status})` : ""}. ${fullRef}`}
    title={status ? `${label} (${status})\n${fullRef}` : `${label}\n${fullRef}`}
    onClick={(event) => { event.stopPropagation(); if (!disabled) onOpen?.(); }}
    onContextMenu={(event) => { event.preventDefault(); void navigator.clipboard.writeText(fullRef); }}
    className="inline-flex min-w-0 max-w-full flex-wrap items-center gap-1 rounded px-1 py-0.5 align-baseline font-mono text-[0.92em] transition-colors hover:brightness-110"
    style={{ color: disabled ? "var(--ct-muted-fg, #8b949e)" : "var(--ct-accent, #6ea8fe)" }}>
    <Icon size={12} strokeWidth={2} className="shrink-0" />
    <span className="min-w-0 whitespace-normal [overflow-wrap:anywhere]">{label}</span>
    {status && <span className="text-[0.82em] opacity-70">({status})</span>}
  </button>;
}
