import { useState, type ReactNode } from "react";
import { Brain, ChevronDown, FileText, Sparkles, Workflow } from "lucide-react";
import { LiquidToggle } from "../shared/LiquidToggle";
import { bringsSummary, groupKeptState, PAYLOAD_GROUPS, payloadItemText, skillScriptPath, visiblePayload } from "../../lib/teamTemplates/startModel";
import { usedByLine } from "../../lib/teamTemplates/exchangeModel";
import { TemplateFileDialog } from "./TemplateFileDialog";

type TemplatePayloadItem = import("../../../gen/TemplatePayloadItem").TemplatePayloadItem;
type TemplateRoleSummary = import("../../../gen/TemplateRoleSummary").TemplateRoleSummary;
type TemplateRef = import("../../../gen/TemplateRef").TemplateRef;
type PayloadKind = import("../../../gen/PayloadKind").PayloadKind;

const AMBER = "var(--ct-yellow,#d97706)";
const BULK_LINK = "ml-auto shrink-0 text-[11px] text-[var(--ct-muted-fg)] hover:text-[var(--ct-accent)] hover:underline";
/** A group's own link stays out of sight until its rows are hovered or focused; the head link is always there. */
const GROUP_LINK = "opacity-0 transition-opacity group-hover/payload:opacity-100 group-focus-within/payload:opacity-100 focus-visible:opacity-100";

const GROUP_ICON: Record<PayloadKind, ReactNode> = {
  file: <FileText size={13} aria-hidden="true" />,
  memory: <Brain size={13} aria-hidden="true" />,
  skill: <Sparkles size={13} aria-hidden="true" />,
  automation: <Workflow size={13} aria-hidden="true" />,
};

/** A quiet text link that keeps every item of a list, or clears them all once they are all kept. */
function BulkToggle({ items, optOut, onToggle, scope, className = "" }: {
  items: readonly TemplatePayloadItem[];
  optOut: ReadonlySet<string>;
  onToggle: (ids: readonly string[], kept: boolean) => void;
  scope: string;
  className?: string;
}) {
  const allKept = groupKeptState(items, optOut) === "all";
  const label = allKept ? "Clear all" : "Select all";
  return <button type="button" className={`${BULK_LINK} ${className}`} aria-label={`${label} ${scope}`}
    onClick={() => onToggle(items.map((item) => item.id), !allKept)}>{label}</button>;
}

/** What a package brings; every item starts on and the owner may opt out. Without `onToggle` it is read-only. */
export function TemplateBrings({ payload, optOut, onToggle, collapsible = true, defaultOpen = !collapsible, title = "Brings", template, roles = [] }: {
  payload: readonly TemplatePayloadItem[];
  /** The package's roles, so "used by" names them as the owner sees them ("Lead"). */
  roles?: readonly TemplateRoleSummary[];
  optOut: ReadonlySet<string>;
  onToggle?: (ids: readonly string[], kept: boolean) => void;
  collapsible?: boolean;
  defaultOpen?: boolean;
  title?: string;
  /** The resolved package whose scripts can be read; a live Team being saved has none yet. */
  template?: TemplateRef;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const [viewing, setViewing] = useState<string | null>(null);
  const items = visiblePayload(payload);
  if (items.length === 0) return null;
  const summary = bringsSummary(items, optOut);
  return (
    <section className="flex flex-col gap-1.5" aria-label={title}>
      <div className="flex items-center gap-2">
        {collapsible ? (
          <button type="button" aria-expanded={open} onClick={() => setOpen(!open)}
            className="flex min-w-0 items-center gap-1.5 text-left text-[12px] text-[var(--ct-fg)]">
            <ChevronDown size={12} aria-hidden="true" className={`shrink-0 ${open ? "" : "-rotate-90"}`} />
            <span className="shrink-0 text-[var(--ct-muted-fg)]">{title}</span>
            <span className="min-w-0 break-words" title={summary} data-testid="brings-summary">{summary}</span>
          </button>
        ) : (
          <div className="flex min-w-0 flex-wrap items-baseline gap-1 text-[12px]">
            <span className="shrink-0 text-[11px] font-medium text-[var(--ct-muted-fg)]">{title}</span>
            <span className="min-w-0 break-words" title={summary} data-testid="brings-summary">{summary}</span>
          </div>
        )}
        {open && onToggle && <BulkToggle items={items} optOut={optOut} onToggle={onToggle} scope={title.toLowerCase()} />}
      </div>
      {open && PAYLOAD_GROUPS.map((group) => {
        const grouped = items.filter((item) => item.kind === group.kind);
        if (grouped.length === 0) return null;
        return (
          <div key={group.kind} role="group" aria-label={group.title} className={`group/payload flex flex-col ${collapsible ? "pl-4" : ""}`}>
            <div className="flex items-center gap-1.5 py-1 text-[11px] font-medium text-[var(--ct-muted-fg)]">
              {GROUP_ICON[group.kind]}<span>{group.title}</span>
              {onToggle && grouped.length > 1 && <BulkToggle items={grouped} optOut={optOut} onToggle={onToggle} scope={group.title.toLowerCase()} className={GROUP_LINK} />}
            </div>
            {grouped.map((item) => {
              const kept = !optOut.has(item.id);
              const usedBy = usedByLine(item, roles);
              const text = payloadItemText(item);
              const script = item.kind === "skill" ? skillScriptPath(item) : item.script_path ?? null;
              const detail = [text.detail?.replace(/\.\s*$/, ""), usedBy].filter(Boolean).join(" · ");
              return (
                <div key={item.id} className="flex items-center gap-3 border-t border-[color-mix(in_srgb,var(--ct-fg)_6%,transparent)] py-1.5 pl-[19px] first-of-type:border-t-0" data-payload-item={item.id}>
                  <div className={`min-w-0 flex-1 ${kept ? "" : "opacity-55"}`}>
                    <div className={`text-[12px] text-[var(--ct-fg)] ${item.kind === "skill" ? "font-mono" : ""}`}>{text.name}</div>
                    {detail && <div className="text-[11px] text-[var(--ct-muted-fg)]">{detail}</div>}
                    {item.executable && (
                      <div className="flex min-w-0 flex-wrap items-center gap-1.5 text-[11px]" data-executable="true">
                        <span aria-hidden="true" className="h-1.5 w-1.5 shrink-0 rounded-full" style={{ background: AMBER }} />
                        <span className="min-w-0 break-words [overflow-wrap:anywhere]" title={item.script_path ?? undefined}>
                          {script ? <>Runs <span className="font-mono">{script}</span></> : "Runs scripts"}
                        </span>
                        {item.script_path && template && <button type="button" className="shrink-0 text-[var(--ct-accent)] hover:underline"
                          aria-label={`View ${item.script_path}`} onClick={() => setViewing(item.script_path ?? null)}>View</button>}
                      </div>
                    )}
                  </div>
                  {onToggle && <LiquidToggle checked={kept} aria-label={`Bring ${text.name}`} onCheckedChange={(next) => onToggle([item.id], next)} />}
                </div>
              );
            })}
          </div>
        );
      })}
      {viewing && template && <TemplateFileDialog template={template} target={{ path: viewing }} onClose={() => setViewing(null)} />}
    </section>
  );
}
