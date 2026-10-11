import { useCallback, useDeferredValue, useEffect, useLayoutEffect, useMemo, useReducer, useRef, type KeyboardEvent, type MouseEvent, type RefObject } from "react";
import {
  clipMessage,
  DOMIOS_TEXT_MIME,
  messagesClipboard,
  messagesExcerpt,
  messagesMarkdown,
  messagesQuote,
  messagesReference,
  messagesReplyLabel,
  timestampMs,
  type ClipboardFlavours,
  type ClipMessage,
  type ClipNames,
  type NormalizedChatMessage,
  type TimelineItem,
} from "@codeterm/chat-core";
import { formatMessageTime, writeClipboardFlavours } from "@codeterm/chat-ui";
import type { MessageRowHandlers } from "./ChatMessageRow";
import { insertIntoComposer } from "./composerInsertion";
import { replyInComposer } from "./composerReply";
import { clipsForParts, indexMessageParts, partSelectionSummary } from "./messagePartSelection";
import { EMPTY_SELECTION, orderedSelection, reduceMessageSelection, selectionActive } from "./messageSelection";

export interface SelectableRow {
  msg: NormalizedChatMessage;
  item: TimelineItem;
}

const INTERACTIVE = "a, button, input, textarea, select, summary, label, [role='button'], [role='checkbox'], [contenteditable='true']";
const GUTTER = "[data-message-gutter]";
/** How far a press in the gutter travels before it becomes a block drag rather than a click. */
const DRAG_SLOP_PX = 4;

export function chatClipNames(agent: string): ClipNames {
  return {
    agent,
    formatTime: (timestamp) => {
      const ms = timestampMs(timestamp);
      return ms == null ? undefined : formatMessageTime(ms);
    },
  };
}

export function rowClip(row: SelectableRow, names: ClipNames): ClipMessage {
  return clipMessage(row.msg, row.item.kind === "tool" ? row.item.event : undefined, names);
}

/** Quote: the messages as attributed blockquotes in the composer's one reply block. */
export function quoteMessages(tabId: string, clips: readonly ClipMessage[]) {
  if (!clips.length) return;
  replyInComposer(tabId, { label: messagesReplyLabel(clips), excerpt: messagesExcerpt(clips), ref: clips[0].id, content: messagesQuote(clips) });
}

/** Ask: the messages' full transcript in one message capsule. */
export function askAboutMessages(tabId: string, clips: readonly ClipMessage[]) {
  if (!clips.length) return;
  const reference = messagesReference(clips);
  insertIntoComposer(tabId, { kind: "message", label: clips.length === 1 ? `msg · ${reference}` : reference, content: messagesMarkdown(clips) });
}

function editable(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && Boolean(target.closest("input, textarea, select, [contenteditable='true']"));
}

function nativeTextSelected(): boolean {
  const selection = typeof window === "undefined" ? null : window.getSelection();
  return Boolean(selection && !selection.isCollapsed && selection.toString().length > 0);
}

function rowIdAt(target: EventTarget | null, surface: HTMLElement | null, y?: number): string | null {
  if (!(target instanceof Element)) return null;
  const row = target.closest<HTMLElement>("[data-message-part-id], [data-message-id]");
  if (!row || (surface && !surface.contains(row))) return null;
  if (row.dataset.messagePartId) return row.dataset.messagePartId;
  const parts = Array.from(row.querySelectorAll<HTMLElement>("[data-message-part-id]"));
  const part = parts.find((element) => y !== undefined && y >= element.getBoundingClientRect().top && y <= element.getBoundingClientRect().bottom) ?? parts[0];
  return part?.dataset.messagePartId ?? row.dataset.messageId ?? null;
}

interface Press {
  id: string;
  y: number;
  dragging: boolean;
}

export function useChatMessageSelection({ tabId, rows, agent, surfaceRef, streamingMessageId, prepareMessage, contentVersion }: {
  tabId: string;
  rows: readonly SelectableRow[];
  agent: string;
  surfaceRef: RefObject<HTMLElement | null>;
  streamingMessageId?: string | null;
  prepareMessage?: (id: string) => boolean;
  contentVersion?: unknown;
}) {
  const [state, dispatch] = useReducer(reduceMessageSelection, EMPTY_SELECTION);
  const renderingStreamId = useDeferredValue(streamingMessageId);
  const index = useMemo(() => indexMessageParts(rows.map((row) => rowClip(row, chatClipNames(agent))), renderingStreamId), [rows, agent, renderingStreamId]);
  const { byMessage: partsByMessage, order } = index;
  const live = useRef({ index, state });
  live.current = { index, state };
  const focusCursor = useRef(false);
  const press = useRef<Press | null>(null);
  const stopTracking = useRef<(() => void) | null>(null);

  useEffect(() => { dispatch({ type: "prune", order }); }, [order]);
  useEffect(() => () => stopTracking.current?.(), []);

  const clipsOf = useCallback((ids: readonly string[]): ClipMessage[] => {
    return clipsForParts(live.current.index, ids);
  }, []);
  const clipFor = useCallback((id: string): ClipMessage | null => live.current.index.byId.get(id) ?? null, []);
  const selectedClips = useCallback(() => clipsOf(orderedSelection(live.current.state, live.current.index.order)), [clipsOf]);

  const clear = useCallback(() => dispatch({ type: "clear" }), []);
  const select = useCallback((id: string) => {
    const ids = live.current.index.byMessage.get(id)?.map((part) => part.id) ?? [id];
    for (const target of ids) if (!live.current.state.selected.includes(target)) dispatch({ type: "toggle", id: target });
    focusCursor.current = true;
  }, []);
  const copySelection = useCallback(() => writeClipboardFlavours(messagesClipboard(selectedClips())), [selectedClips]);
  const quoteSelection = useCallback(() => { quoteMessages(tabId, selectedClips()); dispatch({ type: "clear" }); }, [selectedClips, tabId]);
  const askSelection = useCallback(() => { askAboutMessages(tabId, selectedClips()); dispatch({ type: "clear" }); }, [selectedClips, tabId]);

  const track = useCallback(() => {
    stopTracking.current?.();
    const onMove = (event: globalThis.MouseEvent) => {
      const current = press.current;
      if (!current) return;
      const over = rowIdAt(event.target, surfaceRef.current, event.clientY);
      if (!current.dragging) {
        if (Math.abs(event.clientY - current.y) < DRAG_SLOP_PX && (over === null || over === current.id)) return;
        current.dragging = true;
        dispatch({ type: "dragStart", id: current.id, order: live.current.index.order, extend: false });
      }
      event.preventDefault();
      if (over) dispatch({ type: "dragTo", id: over, order: live.current.index.order });
    };
    const onUp = () => {
      if (press.current?.dragging) dispatch({ type: "dragEnd" });
      press.current = null;
      stop();
    };
    const stop = () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
      if (stopTracking.current === stop) stopTracking.current = null;
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
    stopTracking.current = stop;
  }, [surfaceRef]);

  const rowMouseDown = useCallback((id: string, event: MouseEvent<HTMLElement>) => {
    if (event.button !== 0 || event.ctrlKey || event.metaKey || event.altKey) return;
    const target = event.target as HTMLElement;
    const targetId = rowIdAt(target, surfaceRef.current, event.clientY);
    id = targetId && live.current.index.order.includes(targetId) ? targetId : live.current.index.byMessage.get(id)?.[0]?.id ?? id;
    if (target.closest("[role='menu']")) return;
    const inGutter = Boolean(target.closest(GUTTER));
    if (!inGutter && target.closest(INTERACTIVE)) return;
    const blockMode = selectionActive(live.current.state);
    if (blockMode || (event.shiftKey && !nativeTextSelected())) {
      event.preventDefault();
      dispatch({ type: "dragStart", id, order: live.current.index.order, extend: event.shiftKey && blockMode });
      press.current = { id, y: event.clientY, dragging: true };
      track();
      return;
    }
    if (!inGutter) return;
    if (!target.closest(INTERACTIVE)) event.preventDefault();
    press.current = { id, y: event.clientY, dragging: false };
    track();
  }, [track]);

  const onKeyDown = useCallback((event: KeyboardEvent<HTMLElement>) => {
    const target = event.target as HTMLElement;
    const row = target.closest<HTMLElement>("[data-message-part-id], [data-message-id]");
    if (row === null || target !== row || event.altKey || event.ctrlKey || event.metaKey) return;
    const id = row.dataset.messagePartId ?? live.current.index.byMessage.get(row.dataset.messageId ?? "")?.[0]?.id ?? "";
    if (event.code === "ArrowDown" || event.code === "ArrowUp") {
      event.preventDefault();
      if (live.current.state.cursor !== id) dispatch({ type: "focus", id });
      dispatch({ type: "move", delta: event.code === "ArrowDown" ? 1 : -1, order: live.current.index.order, extend: event.shiftKey });
      focusCursor.current = true;
    } else if (event.code === "Space") {
      event.preventDefault();
      dispatch({ type: "toggle", id });
    } else if (event.code === "Escape" && selectionActive(live.current.state)) {
      event.preventDefault();
      event.stopPropagation();
      dispatch({ type: "clear" });
    }
  }, []);

  useLayoutEffect(() => {
    if (!focusCursor.current || !state.cursor) return;
    const element = surfaceRef.current?.querySelector<HTMLElement>(`[data-message-part-id="${CSS.escape(state.cursor)}"], [data-message-id="${CSS.escape(state.cursor)}"]`);
    if (!element) {
      const messageId = [...live.current.index.byMessage].find(([, parts]) => parts.some((part) => part.id === state.cursor))?.[0];
      if (prepareMessage?.(messageId ?? state.cursor)) return;
    }
    focusCursor.current = false;
    element?.focus({ preventScroll: true });
    element?.scrollIntoView?.({ block: "nearest" });
  }, [state.cursor, state.selected, surfaceRef, prepareMessage, contentVersion]);

  const active = selectionActive(state);
  useEffect(() => {
    if (!active) return;
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.code !== "Escape" || event.defaultPrevented || editable(event.target)) return;
      dispatch({ type: "clear" });
    };
    const onCopy = (event: ClipboardEvent) => {
      if (!event.clipboardData || editable(event.target) || nativeTextSelected()) return;
      const flavours: ClipboardFlavours = messagesClipboard(selectedClips());
      event.preventDefault();
      event.clipboardData.setData("text/plain", flavours.plain);
      event.clipboardData.setData(DOMIOS_TEXT_MIME, flavours.portable);
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("copy", onCopy);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("copy", onCopy);
    };
  }, [active, selectedClips]);

  const { selected, partial, count } = useMemo(() => partSelectionSummary(index, state.selected), [index, state.selected]);
  const rowHandlers = useMemo<MessageRowHandlers>(() => ({
    clipFor,
    partsFor: (id) => partsByMessage.get(id) ?? [],
    selectedFor: (id) => selected.has(id),
    cursorFor: (id) => state.cursor === id,
    joinNextFor: (id) => {
      const at = order.indexOf(id);
      return at >= 0 && selected.has(id) && selected.has(order[at + 1]);
    },
    select,
    toggle: (id) => dispatch({ type: "toggle", id }),
    range: (from, to, start) => {
      if (start) dispatch({ type: "dragStart", id: from, order, extend: false });
      dispatch({ type: "dragTo", id: to, order });
    },
    endRange: () => dispatch({ type: "dragEnd" }),
    rowMouseDown,
    quote: (clip) => quoteMessages(tabId, [clip]),
    ask: (clip) => askAboutMessages(tabId, [clip]),
  }), [clipFor, rowMouseDown, select, tabId, partsByMessage, selected, state.cursor, order]);
  return {
    rowHandlers,
    selected,
    active,
    partial,
    count,
    cursor: state.cursor,
    clear,
    onKeyDown,
    copySelection,
    quoteSelection,
    askSelection,
  };
}

export type ChatMessageSelection = ReturnType<typeof useChatMessageSelection>;
