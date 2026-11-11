import { useState } from "react";
import { ChevronDown } from "lucide-react";
import { LiquidToggle } from "../shared/LiquidToggle";
import { bringsSummary, PAYLOAD_GROUPS } from "../../lib/teamTemplates/startModel";
import { usedByLine } from "../../lib/teamTemplates/exchangeModel";

type TemplatePayloadItem = import("../../../gen/TemplatePayloadItem").TemplatePayloadItem;

const AMBER = "var(--ct-yellow,#d97706)";

/** What a package brings; every item starts on and the owner may opt out. Without `onToggle` it is read-only. */
export function TemplateBrings({ payload, optOut, onToggle, collapsible = true, defaultOpen = !collapsible, title = "Brings" }: {
  payload: readonly TemplatePayloadItem[];
  optOut: ReadonlySet<string>;
  onToggle?: (id: string, kept: boolean) => void;
  collapsible?: boolean;
  defaultOpen?: boolean;
  title?: string;
}) {
  const [open, setOpen] = useState(defaultOpen);
  if (payload.length === 0) return null;
  const summary = bringsSummary(payload, optOut);
  return (
    <section className="flex flex-col gap-1.5" aria-label={title}>
      {collapsible ? (
        <button type="button" aria-expanded={open} onClick={() => setOpen(!open)}
          className="flex items-center gap-1.5 text-left text-[12px] text-[var(--ct-fg)]">
          <ChevronDown size={12} aria-hidden="true" className={open ? "" : "-rotate-90"} />
          <span className="text-[var(--ct-muted-fg)]">{title}</span>
          <span data-testid="brings-summary">{summary}</span>
        </button>
      ) : (
        <div className="text-[12px]"><span className="text-[var(--ct-muted-fg)]">{title} </span><span data-testid="brings-summary">{summary}</span></div>
      )}
      {open && PAYLOAD_GROUPS.map((group) => {
        const items = payload.filter((item) => item.kind === group.kind);
        if (items.length === 0) return null;
        return (
          <div key={group.kind} role="group" aria-label={group.title} className="flex flex-col pl-4">
            <div className="py-1 text-[11px] text-[var(--ct-muted-fg)]">{group.title}</div>
            {items.map((item) => {
              const kept = !optOut.has(item.id);
              const usedBy = usedByLine(item);
              return (
                <div key={item.id} className="flex items-center gap-3 border-t border-[var(--ct-border)] py-1.5" data-payload-item={item.id}>
                  <div className={`min-w-0 flex-1 ${kept ? "" : "opacity-55"}`}>
                    <div className={`text-[12px] text-[var(--ct-fg)] ${item.kind === "skill" ? "font-mono" : ""}`}>{item.label}</div>
                    {(item.description || usedBy) && <div className="text-[11px] text-[var(--ct-muted-fg)]">{[item.description, usedBy].filter(Boolean).join(" · ")}</div>}
                    {item.executable && (
                      <div className="flex items-center gap-1.5 text-[11px]" data-executable="true">
                        <span aria-hidden="true" className="h-1.5 w-1.5 rounded-full" style={{ background: AMBER }} />
                        {item.script_path ? <>Runs <span className="font-mono">{item.script_path}</span></> : "Runs scripts"}
                      </div>
                    )}
                  </div>
                  {onToggle && <LiquidToggle checked={kept} aria-label={`Bring ${item.label}`} onCheckedChange={(next) => onToggle(item.id, next)} />}
                </div>
              );
            })}
          </div>
        );
      })}
    </section>
  );
}
