import { memo, useMemo, useRef, useState, type MouseEvent, type ReactNode } from "react";
import type { ClipMessage } from "@codeterm/chat-core";
import { MessageActionsDot, type DotSide, type MessageMenuActions } from "./MessageActionsMenu";

export interface MessageRowHandlers {
  clipFor: (id: string) => ClipMessage | null;
  select: (id: string) => void;
  rowMouseDown: (id: string, event: MouseEvent<HTMLElement>) => void;
  quote: (clip: ClipMessage) => void;
  ask: (clip: ClipMessage) => void;
}

/** The dot's hit box height; the pointer is kept at its centre. */
const DOT_BOX_PX = 18;

function Gutter({ side, children }: { side: DotSide; children?: ReactNode }) {
  return <div className="ct-msg-gutter" data-side={side} data-message-gutter="">{children}</div>;
}

/** One chat row: the bubble, a quiet actions dot in its gutter that follows the pointer, and its place in a block selection. */
export const ChatMessageRow = memo(function ChatMessageRow({ msg, ariaLabel, selected, joinNext, selecting, cursor, handlers, children }: {
  msg: { id: string; type: string; sourceMessageId?: string };
  ariaLabel: string;
  selected: boolean;
  joinNext: boolean;
  selecting: boolean;
  cursor: boolean;
  handlers: MessageRowHandlers;
  children: ReactNode;
}) {
  const rowRef = useRef<HTMLDivElement>(null);
  const [hot, setHot] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const side: DotSide = msg.type === "user" ? "end" : "start";
  const clip = hot || menuOpen ? handlers.clipFor(msg.id) : null;
  const actions = useMemo<MessageMenuActions>(() => ({
    quote: handlers.quote,
    ask: handlers.ask,
    select: () => handlers.select(msg.id),
  }), [handlers, msg.id]);

  const placeDot = (offset: number | null) => {
    const row = rowRef.current;
    if (!row) return;
    const room = Math.max(row.getBoundingClientRect().height - DOT_BOX_PX, 0);
    const y = offset === null ? 0 : Math.min(Math.max(offset - DOT_BOX_PX / 2, 0), room);
    row.style.setProperty("--ct-msg-dot-y", `${y}px`);
  };

  const onOpenChange = (open: boolean) => {
    setMenuOpen(open);
    if (!open) setHot(rowRef.current?.matches(":hover, :focus-within") ?? false);
  };

  const dot = clip && !selecting ? (
    <MessageActionsDot clip={clip} side={side} open={menuOpen} onOpenChange={onOpenChange} actions={actions} />
  ) : null;

  return (
    <div
      ref={rowRef}
      data-testid="chat-message"
      data-message-role={msg.type}
      data-message-id={msg.id}
      data-source-message-id={msg.sourceMessageId ?? msg.id}
      data-selected={selected || undefined}
      data-join-next={(selected && joinNext) || undefined}
      data-selecting={selecting || undefined}
      data-actions-open={menuOpen || undefined}
      role="article"
      aria-label={ariaLabel}
      tabIndex={cursor ? 0 : -1}
      className="ct-msg-row"
      onPointerEnter={() => setHot(true)}
      onPointerLeave={() => { if (!menuOpen) setHot(false); }}
      onMouseMove={(event) => {
        if (!menuOpen) placeDot(event.clientY - event.currentTarget.getBoundingClientRect().top);
      }}
      onFocus={(event) => {
        if (event.target === event.currentTarget) placeDot(null);
        setHot(true);
      }}
      onBlur={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setHot(false); }}
      onMouseDown={(event) => handlers.rowMouseDown(msg.id, event)}
    >
      <Gutter side="start">{side === "start" && dot}</Gutter>
      <Gutter side="end">{side === "end" && dot}</Gutter>
      {children}
    </div>
  );
});
