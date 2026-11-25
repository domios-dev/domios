import React, { useRef } from "react";
import { Bot, Globe, Terminal } from "lucide-react";
import { FloatingTooltip } from "../FloatingTooltip";
import type { ChatMentionValue } from "../blocks/ChatBlockHost";
import { truncateChipTitle } from "./ReferenceChip";
import "./participantMention.css";

export interface ParticipantMentionProps {
  label: string;
  fullRef: string;
  state?: string;
  disabled: boolean;
  onOpen?: () => void;
  participant?: Pick<ChatMentionValue, "tabKind" | "providerName" | "role" | "peerName" | "workspaceName" | "activity"> & { avatar?: React.ReactNode };
}

export function ParticipantMentionChip({ label, fullRef, state = "resolved", disabled, onOpen, participant }: ParticipantMentionProps) {
  const pointer = useRef({ type: "", tooltip: false });
  if (state === "missing") return <span className="ct-mention-missing" data-ref-kind="tab" data-ref-state="missing">Unknown tab</span>;
  const loading = state === "loading";
  const closed = state === "closed";
  const title = loading ? "Tab" : label;
  const kind = participant?.tabKind ?? "shell";
  const activity = !loading && !closed && kind === "agent" ? participant?.activity : undefined;
  const status = activity === "waiting" ? "waiting for you" : activity === "error" ? "error" : activity === "working" ? "working" : undefined;
  const description = loading ? "loading tab" : closed ? "closed tab" : kind === "agent"
    ? `${participant?.providerName ? `${participant.providerName} ` : ""}agent tab` : kind === "browser" ? "Browser tab" : kind === "surface" ? "surface tab" : "shell tab";
  const role = participant?.role ? participant.role[0].toUpperCase() + participant.role.slice(1) : undefined;
  const tooltip = [title, closed ? "Closed" : loading ? "Loading" : role,
    !loading && !closed ? participant?.providerName : undefined,
    !loading && !closed ? participant?.peerName : undefined,
    !loading && !closed ? participant?.workspaceName : undefined, status].filter(Boolean).join(" · ");
  const Icon = kind === "browser" ? Globe : kind === "agent" ? Bot : Terminal;
  return <FloatingTooltip focusable={false} className="ct-mention-anchor" content={tooltip}
    tooltipStyle={{ width: "max-content", maxWidth: "min(22rem, calc(100vw - 24px))" }}>
    <button type="button" className="ct-participant-mention" data-testid="chat-tab-link" data-ref-kind="tab" data-ref-state={state}
      aria-label={[title, description, status].filter(Boolean).join(", ")} aria-disabled={disabled || closed || loading || undefined}
      onPointerDown={(event) => { pointer.current = { type: event.pointerType, tooltip: false }; }}
      onClick={(event) => { event.stopPropagation(); if (!pointer.current.tooltip && !disabled && !closed && !loading) onOpen?.(); }}
      onContextMenu={(event) => {
        event.preventDefault();
        if (pointer.current.type === "touch") { pointer.current.tooltip = true; event.currentTarget.focus(); }
        else void navigator.clipboard.writeText(fullRef);
      }}>
      <span className="ct-mention-avatar" aria-hidden="true">
        <span className="ct-mention-avatar-icon">{participant?.avatar ?? <Icon size={kind === "agent" ? 12 : 11} />}</span>
      </span>
      <span className="ct-mention-title">{truncateChipTitle(title, 24)}</span>
    </button>
  </FloatingTooltip>;
}
