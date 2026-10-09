import { toolCommandLabel, type ToolEvent } from "@codeterm/chat-core";
import { Check, CircleX, Copy, LoaderCircle, ChevronRight, MessageSquarePlus, Minus, SquareTerminal } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { useChatBlockHost, type ChatBlockHost, type ChatCardMenuItem } from "../blocks/ChatBlockHost";
import { askExcerpt, ChatCardMenu, copyPlainText } from "../ChatCardMenu";
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
  if (output) items.push({ label: "Copy output", icon: <Copy size={MENU_ICON} aria-hidden />, onSelect: () => copyPlainText(output) });
  if (host.askAbout) {
    const askAbout = host.askAbout;
    const summary = plainSummary(event);
    const title = [label, summary].filter(Boolean).join(" ") || "Tool call";
    const text = [
      `About this ${label || "tool"} call: ${summary}`,
      command && command !== summary ? `Command:\n${askExcerpt(command)}` : "",
      output ? `Output:\n${askExcerpt(output)}` : "",
    ].filter(Boolean).join("\n\n");
    items.push({ label: "Ask about this", icon: <MessageSquarePlus size={MENU_ICON} aria-hidden />, onSelect: () => askAbout({ title, text }) });
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

export function ToolCardFrame({ testId = "chat-tool-call", event, expanded, onToggle, children, title, summary, metric, chips, tail, action, variant = "smart", visualOverride, disclosureLabel, menu }: ToolCardFrameProps) {
  const host = useChatBlockHost();
  const visual = visualOverride ?? toolKindVisual[event.kind];
  const Glyph = visual.glyph;
  const statusColor = event.status === "error"
    ? "var(--ct-red)"
    : event.status === "pending" ? "var(--ct-accent)" : event.status === "unresolved" || event.status === "interrupted" ? "var(--ct-muted-fg)" : "var(--ct-green)";
  const error = event.status === "error";
  const quiet = !error && !expanded && variant !== "codeterm";
  const label = title === undefined ? visual.label : title;
  const detailsId = `chat-details-${event.id.replace(/[^a-zA-Z0-9_-]/g, "-")}`;
  const labelText = typeof label === "string" && label ? label : visual.label;
  const menuItems = menu ?? toolMenuItems(event, labelText, host);

  return (
    <div
      data-testid={testId}
      data-tool-status={event.status}
      className={`ct-card group/toolcard mx-auto mb-1.5 w-full max-w-4xl min-w-0 overflow-hidden${quiet ? " ct-card--quiet" : ""}${error ? " ct-card--error" : ""}`}
      style={{ "--ct-card-tone": error ? "var(--ct-muted-fg)" : visual.color } as CSSProperties}
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
          <span className="ct-card__icon" style={{ color: visual.color }}>
            <Glyph aria-hidden="true" size={14} strokeWidth={2} />
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
        <ChatCardMenu label={`Actions for ${labelText || "tool call"}`} items={menuItems} />
        <ChevronRight aria-hidden="true" size={14} className={expanded ? "rotate-90" : ""} style={{ color: "var(--ct-muted-fg)" }} />
      </div>
      <div
        id={detailsId}
        role="region"
        aria-label={disclosureLabel ?? "Tool details"}
        hidden={!expanded}
        className="pb-2 pl-12 pr-3 pt-0.5"
        style={{ boxSizing: "border-box", maxHeight: "600px", overflowY: "auto" }}
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

export function ToolCardActionButton({ label, onClick, testId, primary = false }: { label: string; onClick: () => void; testId?: string; primary?: boolean }) {
  return (
    <button
      type="button"
      data-testid={testId}
      title={label}
      onClick={onClick}
      className={`ct-bar-pill w-fit${primary ? " ct-bar-pill--primary" : ""}`}
      style={{ height: 22 }}
    >
      {label}
    </button>
  );
}
