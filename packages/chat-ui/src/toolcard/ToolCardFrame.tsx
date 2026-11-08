import { ExpandableReader } from "../ExpandableReader";
import { BoundedMessageContent } from "../BoundedMessageContent";
import { toolCommandLabel, type ToolEvent } from "@codeterm/chat-core";
import { ArrowUpRight, Copy, MessageSquarePlus, SquareTerminal } from "lucide-react";
import { useCompactToolRow } from "../useCompactToolRow";
import { CardChevron } from "../CompactCard";
import { RowMenuIsolate } from "../messageMenu";
import { useId, useState, type CSSProperties, type ReactNode } from "react";
import { useChatBlockHost, type ChatBlockHost, type ChatCardMenuItem } from "../blocks/ChatBlockHost";
import { askExcerpt, ChatCardMenu, copyPlainText } from "../ChatCardMenu";
import type { ToolKindVisual } from "./toolKindVisual";
import { toolEventVisual, toolOutcomeVisual, toolVisualForOutcome } from "./toolVisualModel";
import { ToolOutcomeFacts } from "./ToolOutcomeFacts";
import { rawToolPayload } from "./toolParsing";

interface ToolCardFrameProps {
  expandable?: boolean;
  communication?: boolean;
  showTechnicalDetails?: boolean;
  headerTestId?: string;
  statusTestId?: string;
  statusLabel?: string;
  minHeight?: number;
  testId?: string;
  event: ToolEvent;
  expanded: boolean;
  onToggle: () => void;
  children: ReactNode;
  primaryContent?: ReactNode;
  title?: ReactNode;
  subtitle?: ReactNode;
  summary?: ReactNode;
  metric?: ReactNode;
  chips?: ReactNode;
  tail?: ReactNode;
  action?: ReactNode;
  variant?: "smart" | "codeterm";
  visualOverride?: ToolKindVisual;
  disclosureLabel?: string;
  /** Overflow items in place of the tool defaults (copy command, copy output, ask). */
  menu?: readonly ChatCardMenuItem[];
}

const MENU_ICON = 14;

function plainSummary(event: ToolEvent): string {
  return (event.description ?? event.summary ?? "").trim();
}

function toolMenuItems(event: ToolEvent, label: string, host: ChatBlockHost): ChatCardMenuItem[] {
  const command = event.kind === "command" || event.kind === "codeterm" ? toolCommandLabel(event).trim() : "";
  const output = event.result?.trim() ?? "";
  const items: ChatCardMenuItem[] = [];
  if (command) items.push({ label: "Copy command", icon: <SquareTerminal size={MENU_ICON} aria-hidden />, onSelect: () => copyPlainText(command) });
  if (output) items.push({ label: "Copy output", kind: "copy", icon: <Copy size={MENU_ICON} aria-hidden />, onSelect: () => copyPlainText(output) });
  if (host.askAbout) {
    const askAbout = host.askAbout;
    const summary = plainSummary(event);
    const title = [label, summary].filter(Boolean).join(" ") || "Tool call";
    const text = [
      `About this ${label || "tool"} call: ${summary}`,
      command && command !== summary ? `Command:\n${askExcerpt(command)}` : "",
      output ? `Output:\n${askExcerpt(output)}` : "",
    ].filter(Boolean).join("\n\n");
    items.push({ label: "Ask about this", kind: "ask", icon: <MessageSquarePlus size={MENU_ICON} aria-hidden />, onSelect: () => askAbout({ title, text }) });
  }
  return items;
}

export function ToolCardChips({ chips, wrap = false }: { chips: readonly string[]; wrap?: boolean }) {
  if (chips.length === 0) return null;
  return (
    <span className={`flex min-w-0 gap-1 ${wrap ? "flex-wrap" : ""}`}>
      {chips.map((chip) => <span key={chip} className="ct-card__chip">{chip}</span>)}
    </span>
  );
}

export function ToolCardFrame({ testId = "chat-tool-call", event, expanded, onToggle, children, primaryContent, title, subtitle, summary, metric, chips, tail, action, variant = "smart", visualOverride, disclosureLabel, menu, minHeight, communication = false, showTechnicalDetails = true, headerTestId, statusTestId = "toolcard-status", statusLabel, expandable = true }: ToolCardFrameProps) {
  const host = useChatBlockHost();
  const compact = useCompactToolRow();
  const [technicalOpen, setTechnicalOpen] = useState(false);
  const canExpand = expandable || technicalOpen;
  const open = expanded && canExpand;
  const visual = visualOverride ? toolVisualForOutcome(visualOverride, event.status) : toolEventVisual(event);
  const Glyph = visual.glyph;
  const outcome = toolOutcomeVisual[event.status];
  const StatusGlyph = outcome.glyph;
  const error = event.status === "error";
  const inlineCommand = event.kind === "command";
  const quietOperation = event.kind === "read" || event.kind === "search";
  const inlineRow = inlineCommand || quietOperation;
  const iconColor = quietOperation ? "var(--ct-muted-fg)" : visual.color;
  const statusColor = event.status === "success" ? "var(--ct-muted-fg)" : outcome.color;
  const statusIcon = <span data-testid={statusTestId} role="status" className="ct-tool-status-icon flex w-4 shrink-0 items-center justify-center" aria-label={statusLabel ?? (event.status === "unresolved" ? "no result" : event.status)} style={{ color: statusColor }}>
    <StatusGlyph aria-hidden="true" size={14} className={event.status === "pending" ? "motion-safe:animate-spin" : undefined} />
  </span>;
  const quiet = !error && !open && variant !== "codeterm";
  const label = title === undefined ? visual.label : title;
  const detailsId = `chat-details-${useId().replace(/[^a-zA-Z0-9_-]/g, "-")}`;
  const labelText = typeof label === "string" && label ? label : visual.label;
  const baseMenu = menu ?? toolMenuItems(event, labelText, host);
  const menuItems: readonly ChatCardMenuItem[] = showTechnicalDetails ? [...baseMenu, {
    label: "Details",
    kind: "details",
    icon: <SquareTerminal size={14} aria-hidden />,
    checked: technicalOpen,
    onSelect: () => { setTechnicalOpen(value => !value); if (!expanded) onToggle(); },
  }] : baseMenu;

  return (
    <ExpandableReader label={labelText} enabled={canExpand} openerPlacement="header" ownsMenu>{(fullscreen, opener, _openReader, readerItems) => (
    <div
      data-testid={testId}
      data-tool-status={event.status}
      data-tool-kind={event.kind}
      data-expanded={open}
      data-compact={compact}
      className={`ct-card group/toolcard mx-auto mb-1.5 w-full max-w-4xl min-w-0 overflow-hidden${communication ? " ct-communication-card" : ""}${inlineRow ? " ct-command-inline" : ""}${quiet ? " ct-card--quiet" : ""}${error ? " ct-card--error" : ""}`}
      style={{ minHeight, "--ct-card-tone": error || quietOperation ? "var(--ct-muted-fg)" : visual.color } as CSSProperties}
    >
      <div data-testid={headerTestId} className={`ct-card__header flex min-w-0 items-center gap-2.5 px-3 py-2 text-left text-xs${inlineRow ? " ct-command-inline__head" : ""}`}
        onClick={compact && canExpand ? (event) => {
          if (event.target instanceof Element && !event.target.closest('button, a, input, select, textarea, [role="button"]')) onToggle();
        } : undefined}>
        <button
          type="button"
          aria-label={disclosureLabel ? `${expanded ? "Hide" : "Show"} ${disclosureLabel}` : expanded ? "Collapse tool details" : "Expand tool details"}
          aria-expanded={canExpand ? open : undefined}
          aria-controls={canExpand ? detailsId : undefined}
          disabled={!canExpand}
          onClick={onToggle}
          className={`flex min-w-0 flex-1 gap-2 text-left ${subtitle ? "items-start" : "items-center"}`}
        >
          <span className="ct-card__icon" style={{ color: iconColor, ...(subtitle ? { height: 20, alignSelf: "flex-start" as const } : {}) }}>
            <Glyph aria-hidden="true" size={14} strokeWidth={2} style={{ color: iconColor }} />
          </span>
          <span className={subtitle ? "ct-tool-labels flex min-w-0 flex-1 flex-col items-start gap-0 font-mono text-[12.5px] leading-[18px]" : "ct-tool-labels flex min-w-0 flex-1 items-baseline gap-2 font-mono text-[12.5px]"}>
            {label && <span className={inlineCommand ? "sr-only" : `max-w-[60%] shrink-0 truncate ${quietOperation ? "font-medium" : "font-semibold"}`} style={{ color: communication || quietOperation ? "var(--ct-fg)" : visual.color }}>{label}</span>}
            {subtitle && <span className="max-w-full truncate text-[10.5px] font-normal leading-[14px]" style={{ color: "color-mix(in srgb, var(--ct-muted-fg) 72%, transparent)" }}>{subtitle}</span>}
            <span className={summary === "" ? "hidden" : "ct-tool-subject min-w-0 truncate"} data-testid="toolcard-summary" style={{ color: "var(--ct-fg)" }}>{summary === "" ? null : (summary ?? event.description ?? event.summary) || "Tool activity"}</span>
            {metric ?? chips}
            <ToolOutcomeFacts event={event} />
            {!expanded && tail}
          </span>
        </button>
        {opener}
        {event.status === "unresolved" && (
          <span data-testid="toolcard-no-result" className="shrink-0 font-mono text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>no result</span>
        )}
        {action}
        {compact && event.status !== "success" && statusIcon}
        <span className="ct-tool-status-slot">
        {!compact && statusIcon}
        <ChatCardMenu label={`Actions for ${labelText || "tool call"}`} items={[...readerItems, ...menuItems]} joinRow />
        </span>
        {canExpand && <CardChevron expanded={open} label={open ? "Collapse card" : "Expand card"} onToggle={onToggle} />}
      </div>
      {primaryContent && <div className="px-3 pb-2">{primaryContent}</div>}
      <BoundedMessageContent collapsedHeight={communication ? "160px" : "600px"}
        id={detailsId}
        role="region"
        aria-label={disclosureLabel ?? "Tool details"}
        hidden={!open && !fullscreen}
        className={communication || inlineRow ? "px-3 pb-3 pt-0.5" : "pb-2 pl-12 pr-3 pt-0.5"}
        style={{ boxSizing: "border-box" }}
      >
        {open || fullscreen ? (
          <RowMenuIsolate>
            {children}
            {technicalOpen && showTechnicalDetails && <><div className="mt-2 text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>Command</div><BoundedMessageContent collapsedHeight="12rem" as="pre"
              data-testid="message-raw-content" className="mt-2 whitespace-pre-wrap break-words font-mono text-xs"
              style={{ color: "var(--ct-muted-fg)" }}>{event.detail || rawToolPayload(event)}</BoundedMessageContent></>}
          </RowMenuIsolate>
        ) : null}
      </BoundedMessageContent>
    </div>
    )}</ExpandableReader>
  );
}

export function ToolCardActionButton({ label, onClick, testId, primary = false }: { label: string; onClick: () => void; testId?: string; primary?: boolean }) {
  return (
    <button
      type="button"
      data-testid={testId}
      title={label}
      onClick={onClick}
      className={primary ? "ct-bar-pill ct-bar-pill--primary w-fit" : "ct-tool-action"}
      style={primary ? { height: 22 } : undefined}
    >
      {label}
      {!primary && <ArrowUpRight aria-hidden="true" size={12} />}
    </button>
  );
}
