import React from "react";
import { ClipboardList, FileText, ListChecks, Terminal, Workflow } from "lucide-react";

export const TASK_CHIP_TITLE_CHARS = 36;

/** A task label reads "KEY title"; the key shows once even when the title already repeats it. */
export function splitTaskLabel(label: string): { key: string; title: string } {
  const [key = "", ...rest] = label.trim().split(/\s+/);
  while (rest[0] === key) rest.shift();
  return { key, title: rest.join(" ") };
}

export function truncateChipTitle(title: string, max = TASK_CHIP_TITLE_CHARS): string {
  return title.length <= max ? title : `${title.slice(0, max - 1).trimEnd()}…`;
}

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
  const task = kind === "task" ? splitTaskLabel(label) : null;
  const full = task ? [task.key, task.title].filter(Boolean).join(" ") : label;
  return <button type="button" data-testid={`chat-${kind}-link`} data-ref-kind={kind} data-ref-state={state ?? "resolved"}
    data-target-id={targetId} data-plan-id={planId} disabled={disabled}
    aria-label={`${kind[0].toUpperCase() + kind.slice(1)} ${full}${status ? ` (${status})` : ""}. ${fullRef}`}
    title={status ? `${full} (${status})\n${fullRef}` : `${full}\n${fullRef}`}
    onClick={(event) => { event.stopPropagation(); if (!disabled) onOpen?.(); }}
    onContextMenu={(event) => { event.preventDefault(); void navigator.clipboard.writeText(fullRef); }}
    className="inline-flex max-w-full items-baseline gap-1 rounded px-1 py-0.5 align-baseline font-mono text-[0.92em] font-normal whitespace-nowrap text-start transition-colors hover:brightness-110"
    style={{ color: disabled ? "var(--ct-muted-fg, #8b949e)" : "var(--ct-accent, #6ea8fe)" }}>
    <Icon size={12} strokeWidth={2} className="shrink-0 self-center" />
    {task
      ? <>
        <span className="shrink-0">{task.key}</span>
        {task.title && <span className="min-w-0 max-w-[28ch] truncate">{truncateChipTitle(task.title)}</span>}
      </>
      : <span className="min-w-0 whitespace-normal [overflow-wrap:anywhere]">{label}</span>}
    {status && <span className="shrink-0 text-[0.82em] opacity-70">({status})</span>}
  </button>;
}
