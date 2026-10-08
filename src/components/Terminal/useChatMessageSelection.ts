import { useCallback, useEffect, useLayoutEffect, useMemo, useReducer, useRef, type KeyboardEvent, type MouseEvent, type RefObject } from "react";
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

function rowIdAt(target: EventTarget | null, surface: HTMLElement | null): string | null {
  if (!(target instanceof Element)) return null;
  const row = target.closest<HTMLElement>("[data-message-id]");
  return row && (!surface || surface.contains(row)) ? row.dataset.messageId ?? null : null;
}

interface Press {
  id: string;
  y: number;
  dragging: boolean;
}

/**
 * Block selection of whole chat rows. Shift+click, the Select action, or a press-and-drag in the gutter enters it;
 * while it is on, a press or drag on any row picks rows instead of text. Esc leaves it.
 */
export function useChatMessageSelection({ tabId, rows, agent, surfaceRef }: {
  tabId: string;
  rows: readonly SelectableRow[];
  agent: string;
  surfaceRef: RefObject<HTMLElement | null>;
}) {
  const [state, dispatch] = useReducer(reduceMessageSelection, EMPTY_SELECTION);
  const order = useMemo(() => rows.map(({ msg }) => msg.id), [rows]);
  const live = useRef({ rows, order, names: chatClipNames(agent), state });
  live.current = { rows, order, names: chatClipNames(agent), state };
  const focusCursor = useRef(false);
  const press = useRef<Press | null>(null);
  const stopTracking = useRef<(() => void) | null>(null);

  useEffect(() => { dispatch({ type: "prune", order }); }, [order]);
  useEffect(() => () => stopTracking.current?.(), []);

  const clipsOf = useCallback((ids: readonly string[]): ClipMessage[] => {
    const byId = new Map(live.current.rows.map((row) => [row.msg.id, row]));
    return ids.flatMap((id) => {
      const row = byId.get(id);
      return row ? [rowClip(row, live.current.names)] : [];
    });
  }, []);
  const clipFor = useCallback((id: string): ClipMessage | null => clipsOf([id])[0] ?? null, [clipsOf]);
  const selectedClips = useCallback(() => clipsOf(orderedSelection(live.current.state, live.current.order)), [clipsOf]);

  const clear = useCallback(() => dispatch({ type: "clear" }), []);
  const select = useCallback((id: string) => {
    if (!live.current.state.selected.includes(id)) dispatch({ type: "toggle", id });
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
      const over = rowIdAt(event.target, surfaceRef.current);
      if (!current.dragging) {
        if (Math.abs(event.clientY - current.y) < DRAG_SLOP_PX && (over === null || over === current.id)) return;
        current.dragging = true;
        dispatch({ type: "dragStart", id: current.id, order: live.current.order, extend: false });
      }
      event.preventDefault();
      if (over) dispatch({ type: "dragTo", id: over, order: live.current.order });
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
    if (target.closest("[role='menu']")) return;
    const inGutter = Boolean(target.closest(GUTTER));
    if (!inGutter && target.closest(INTERACTIVE)) return;
    const blockMode = selectionActive(live.current.state);
    if (blockMode || (event.shiftKey && !nativeTextSelected())) {
      event.preventDefault();
      dispatch({ type: "dragStart", id, order: live.current.order, extend: event.shiftKey && blockMode });
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
    const row = target.closest<HTMLElement>("[data-message-id]");
    if (row === null || target !== row || event.altKey || event.ctrlKey || event.metaKey) return;
    const id = row.dataset.messageId ?? "";
    if (event.code === "ArrowDown" || event.code === "ArrowUp") {
      event.preventDefault();
      if (live.current.state.cursor !== id) dispatch({ type: "focus", id });
      dispatch({ type: "move", delta: event.code === "ArrowDown" ? 1 : -1, order: live.current.order, extend: event.shiftKey });
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
    focusCursor.current = false;
    const element = surfaceRef.current?.querySelector<HTMLElement>(`[data-message-id="${CSS.escape(state.cursor)}"]`);
    element?.focus({ preventScroll: true });
    element?.scrollIntoView?.({ block: "nearest" });
  }, [state.cursor, state.selected, surfaceRef]);

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

  const selected = useMemo(() => new Set(state.selected), [state.selected]);
  const rowHandlers = useMemo<MessageRowHandlers>(() => ({
    clipFor,
    select,
    rowMouseDown,
    quote: (clip) => quoteMessages(tabId, [clip]),
    ask: (clip) => askAboutMessages(tabId, [clip]),
  }), [clipFor, rowMouseDown, select, tabId]);
  return {
    rowHandlers,
    selected,
    active,
    count: state.selected.length,
    cursor: state.cursor,
    clear,
    onKeyDown,
    copySelection,
    quoteSelection,
    askSelection,
  };
}

export type ChatMessageSelection = ReturnType<typeof useChatMessageSelection>;
