import { memo, useMemo, useRef, useState, type MouseEvent, type ReactNode } from "react";
import { MessagePartsContext, type MessagePart, messageTimeLabels } from "@codeterm/chat-ui";
import type { ClipMessage } from "@codeterm/chat-core";
import { MessageActionsDot, type DotSide, type MessageMenuActions } from "./MessageActionsMenu";
import { useMessageActionScope } from "./useMessageActionScope";
import { useMessageActionHint } from "./useMessageActionHint";
import { useMessageTouch } from "./useMessageTouch";

export interface MessageRowHandlers {
  clipFor: (id: string) => ClipMessage | null;
  partsFor: (id: string) => readonly MessagePart[];
  selectedFor: (id: string) => boolean;
  cursorFor: (id: string) => boolean;
  joinNextFor: (id: string) => boolean;
  select: (id: string) => void;
  rowMouseDown: (id: string, event: MouseEvent<HTMLElement>) => void;
  toggle: (id: string) => void;
  range: (from: string, to: string, start: boolean) => void;
  endRange: () => void;
  quote: (clip: ClipMessage) => void;
  ask: (clip: ClipMessage) => void;
}

/** The dot's hit box height; the pointer is kept at its centre. */
const DOT_BOX_PX = 18;
const TIME_ZONE = Intl.DateTimeFormat().resolvedOptions().timeZone;

function Gutter({ side, children }: { side: DotSide; children?: ReactNode }) {
  return <div className="ct-msg-gutter" data-side={side} data-message-gutter="">{children}</div>;
}

/** One chat row: the bubble, a quiet actions dot in its gutter that follows the pointer, and its place in a block selection. */
export const ChatMessageRow = memo(function ChatMessageRow({ msg, timestamp, deliveryState, sourceOrdinal, ariaLabel, selected, joinNext, selecting, cursor, handlers, onDelete, children }: {
  msg: { id: string; type: string; sourceMessageId?: string };
  timestamp?: number;
  deliveryState?: string;
  sourceOrdinal?: number;
  ariaLabel: string;
  selected: boolean;
  joinNext: boolean;
  selecting: boolean;
  cursor: boolean;
  handlers: MessageRowHandlers;
  children: ReactNode;
  onDelete?: () => void;
}) {
  const rowRef = useRef<HTMLDivElement>(null);
  const [hot, setHot] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [touchMenu, setTouchMenu] = useState(false);
  const [partId, setPartId] = useState<string | null>(null);
  const [whole, setWhole] = useMessageActionScope();
  const parts = handlers.partsFor(msg.id);
  const targetId = whole ? msg.id : partId && parts.some((part) => part.id === partId) ? partId : parts[0]?.id ?? msg.id;
  const fullClip = handlers.clipFor(msg.id);
  const side: DotSide = "end";
  const hint = useMessageActionHint(rowRef, selecting || menuOpen);
  const clip = hot || menuOpen ? handlers.clipFor(targetId) : null;
  const actions = useMemo<MessageMenuActions>(() => ({
    delete: onDelete,
    quote: handlers.quote,
    ask: handlers.ask,
    select: () => handlers.select(targetId),
  }), [handlers, targetId, onDelete]);
  useMessageTouch(rowRef, {
    selecting,
    open: (id) => { setPartId(id); setTouchMenu(true); setMenuOpen(true); hint.opened(); },
    toggle: handlers.toggle,
    range: handlers.range,
    endRange: handlers.endRange,
  });

  const placeDot = (offset: number | null) => {
    const row = rowRef.current;
    if (!row) return;
    const room = Math.max(row.getBoundingClientRect().height - DOT_BOX_PX, 0);
    const y = offset === null ? 0 : Math.min(Math.max(offset - DOT_BOX_PX / 2, 0), room);
    row.style.setProperty("--ct-msg-dot-y", `${y}px`);
  };

  const onOpenChange = (open: boolean) => {
    if (open && !menuOpen) hint.opened();
    setMenuOpen(open);
    if (!open) setTouchMenu(false);
    if (!open) setHot(rowRef.current?.matches(":hover, :focus-within") ?? false);
  };

  const time = timestamp == null || (!hot && !menuOpen) ? null : messageTimeLabels(timestamp, { timeZone: TIME_ZONE, now: Date.now() });
  const dot = clip && (!selecting || menuOpen) ? (
    <MessageActionsDot metadata={time ? { ...time, sender: fullClip?.author, deliveryState } : undefined} touch={touchMenu} whole={whole} hasParts={parts.length > 1} onScopeChange={setWhole} clip={clip} side={side} open={menuOpen} onOpenChange={onOpenChange} actions={actions} hint={hint.hint} onHintEnd={hint.finish} />
  ) : null;

  return (
    <div
      ref={rowRef}
      data-testid="chat-message"
      data-message-role={msg.type}
      data-message-id={msg.id}
      data-source-message-id={msg.sourceMessageId ?? msg.id}
      data-source-message-ordinal={sourceOrdinal}
      data-selected={selected || undefined}
      data-join-next={(selected && joinNext) || undefined}
      data-selecting={selecting || undefined}
      data-has-parts={parts.length > 1 || undefined}
      data-actions-open={menuOpen || undefined}
      data-preview-whole={menuOpen && (whole || parts.length <= 1) || undefined}
      role="article"
      aria-label={ariaLabel}
      tabIndex={cursor ? 0 : -1}
      className="ct-msg-row"
      onPointerEnter={(event) => { if (event.pointerType === "mouse") { setHot(true); hint.enter(); } }}
      onPointerMove={() => hint.move()}
      onPointerLeave={() => { hint.leave(); if (!menuOpen) setHot(false); }}
      onMouseMove={(event) => {
        if (menuOpen) return;
        const elements = Array.from(event.currentTarget.querySelectorAll<HTMLElement>("[data-message-part-id]"));
        const part = elements.find((element) => {
          const rect = element.getBoundingClientRect();
          return event.clientY >= rect.top && event.clientY <= rect.bottom;
        });
        if (part) {
          setPartId(part.dataset.messagePartId ?? null);
          placeDot(part.getBoundingClientRect().top - event.currentTarget.getBoundingClientRect().top + DOT_BOX_PX / 2);
        } else if (!elements.length) placeDot(event.clientY - event.currentTarget.getBoundingClientRect().top);
      }}
      onFocus={(event) => {
        if (!menuOpen) {
          const part = (event.target as HTMLElement).closest<HTMLElement>("[data-message-part-id]");
          if (part) {
            setPartId(part.dataset.messagePartId ?? null);
            placeDot(part.getBoundingClientRect().top - event.currentTarget.getBoundingClientRect().top + DOT_BOX_PX / 2);
          } else if (event.target === event.currentTarget) {
            setPartId(null);
            placeDot(null);
          }
        }
        setHot(true);
      }}
      onBlur={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setHot(false); }}
      onContextMenu={onDelete ? (event) => { event.preventDefault(); setPartId(null); setHot(true); setMenuOpen(true); hint.opened(); } : undefined}
      onKeyDown={(event) => {
        if (onDelete && selected && event.code === "Delete" && event.target === event.currentTarget
          && !event.altKey && !event.ctrlKey && !event.metaKey) {
          event.preventDefault(); event.stopPropagation(); onDelete();
        }
      }}
      onMouseDown={(event) => handlers.rowMouseDown(msg.id, event)}
    >
      <Gutter side="start" />
      <Gutter side="end">
        {dot}
        {selecting && parts.length <= 1 && <span aria-hidden="true" className="ct-msg-bead" data-selected={selected || undefined} />}
        {selecting && parts.length <= 1 && selected && joinNext && <span aria-hidden="true" className="ct-msg-thread" />}
      </Gutter>
      <MessagePartsContext.Provider value={fullClip?.role === "assistant" ? {
        id: msg.id, source: fullClip.source, selecting: selecting && parts.length > 1,
        selected: handlers.selectedFor, joinNext: handlers.joinNextFor, cursor: handlers.cursorFor,
        preview: menuOpen && !whole && parts.length > 1 ? targetId : null,
      } : null}>{children}</MessagePartsContext.Provider>
    </div>
  );
});
