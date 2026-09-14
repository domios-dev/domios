import type { ToolEvent } from "@codeterm/chat-core";
import { Check, CircleX, LoaderCircle, ChevronRight, Minus } from "lucide-react";
import type { ReactNode } from "react";
import { RawCommandDisclosure } from "../codeterm/RawCommandDisclosure";
import { toolKindVisual, type ToolKindVisual } from "./toolKindVisual";
import { ToolOutcomeFacts } from "./ToolOutcomeFacts";

interface ToolCardFrameProps {
  testId?: string;
  event: ToolEvent;
  expanded: boolean;
  onToggle: () => void;
  children: ReactNode;
  title?: ReactNode;
  summary?: ReactNode;
  metric?: ReactNode;
  chips?: ReactNode;
  tail?: ReactNode;
  action?: ReactNode;
  variant?: "smart" | "codeterm";
  visualOverride?: ToolKindVisual;
  disclosureLabel?: string;
}

export function ToolCardFrame({ testId = "chat-tool-call", event, expanded, onToggle, children, title, summary, metric, chips, tail, action, variant = "smart", visualOverride, disclosureLabel }: ToolCardFrameProps) {
  const visual = visualOverride ?? toolKindVisual[event.kind];
  const Glyph = visual.glyph;
  const statusColor = event.status === "error"
    ? "var(--ct-red)"
    : event.status === "pending" ? "var(--ct-yellow)" : event.status === "unresolved" || event.status === "interrupted" ? "var(--ct-muted-fg)" : "var(--ct-green)";
  const borderColor = event.status === "error"
    ? "color-mix(in srgb, var(--ct-red) 38%, var(--ct-border))"
    : expanded || variant === "codeterm" ? "var(--ct-border)" : "transparent";
  const background = event.status === "error"
    ? "color-mix(in srgb, var(--ct-red) 6%, var(--ct-surface))"
    : expanded || variant === "codeterm" ? "var(--ct-surface)" : "color-mix(in srgb, var(--ct-surface) 60%, transparent)";
  const label = title === undefined ? visual.label : title;
  const detailsId = `chat-details-${event.id.replace(/[^a-zA-Z0-9_-]/g, "-")}`;

  return (
    <div
      data-testid={testId}
      data-tool-status={event.status}
      className="group/toolcard mx-auto mb-1.5 w-full max-w-4xl min-w-0 overflow-hidden rounded-lg border transition-colors hover:border-[var(--ct-border)] hover:bg-[var(--ct-surface)]"
      style={{ borderStyle: event.status === "error" ? "dashed" : "solid", borderColor, background }}
    >
      <div className="flex min-w-0 items-center gap-2.5 px-3 py-2 text-left text-xs">
        <button
          type="button"
          aria-label={disclosureLabel ? `${expanded ? "Hide" : "Show"} ${disclosureLabel}` : expanded ? "Collapse tool details" : "Expand tool details"}
          aria-expanded={expanded}
          aria-controls={detailsId}
          onClick={onToggle}
          className="flex min-w-0 flex-1 items-center gap-2.5 text-left"
        >
          <span
            className={`flex ${variant === "codeterm" ? "h-[26px] w-[26px] rounded-[7px] border" : "h-6 w-6 rounded-md"} shrink-0 items-center justify-center`}
            style={{
              color: visual.color,
              background: `color-mix(in srgb, ${visual.color} 15%, transparent)`,
              borderColor: variant === "codeterm" ? `color-mix(in srgb, ${visual.color} 34%, transparent)` : undefined,
            }}
          >
            <Glyph aria-hidden="true" size={variant === "codeterm" ? 15 : 14} strokeWidth={2} />
          </span>
          <span className="flex min-w-0 flex-1 items-baseline gap-2 font-mono text-[12.5px]">
            {label && <span className="shrink-0 font-semibold" style={{ color: visual.color }}>{label}</span>}
            <span className="min-w-0 truncate" data-testid="toolcard-summary" style={{ color: "var(--ct-fg)" }}>{(summary ?? event.description ?? event.summary) || "Tool activity"}</span>
            {metric ?? chips}
            <ToolOutcomeFacts event={event} />
            {!expanded && tail}
          </span>
        </button>
        {event.status === "unresolved" && (
          <span data-testid="toolcard-no-result" className="shrink-0 font-mono text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>no result</span>
        )}
        <span data-testid="toolcard-status" aria-label={event.status === "unresolved" ? "no result" : event.status} style={{ color: statusColor }}>
          {event.status === "pending"
            ? <LoaderCircle className="animate-spin" size={14} />
            : event.status === "error" ? <CircleX size={14} /> : event.status === "unresolved" || event.status === "interrupted" ? <Minus size={14} /> : <Check size={14} />}
        </span>
        {action}
        <ChevronRight aria-hidden="true" size={14} className={expanded ? "rotate-90" : ""} style={{ color: "var(--ct-muted-fg)" }} />
      </div>
      <div
        id={detailsId}
        role="region"
        aria-label={disclosureLabel ?? "Tool details"}
        hidden={!expanded}
        className={`${variant === "codeterm" ? "border-t" : ""} pb-2 pl-[46px] pr-3 pt-0.5`}
        style={{ borderColor: "var(--ct-border)", boxSizing: "border-box", maxHeight: "600px", overflowY: "auto" }}
      >
        {expanded ? (
          <RawCommandDisclosure raw={event.detail || event.summary || ""}>
            {children}
          </RawCommandDisclosure>
        ) : null}
      </div>
    </div>
  );
}

export function ToolCardActionButton({ label, onClick, testId }: { label: string; onClick: () => void; testId?: string }) {
  return (
    <button
      type="button"
      data-testid={testId}
      title={label}
      onClick={onClick}
      className="shrink-0 rounded border px-2 py-0.5 font-mono text-[10.5px] transition-colors hover:brightness-125"
      style={{ borderColor: "var(--ct-border)", color: "var(--ct-muted-fg)", background: "transparent" }}
    >
      {label}
    </button>
  );
}
