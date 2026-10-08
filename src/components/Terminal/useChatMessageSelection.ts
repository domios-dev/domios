import { useCallback, useEffect, useLayoutEffect, useMemo, useReducer, useRef, type KeyboardEvent, type MouseEvent, type RefObject } from "react";
import {
  clipMessage,
  DOMIOS_TEXT_MIME,
  messagesClipboard,
  messagesMarkdown,
  messagesQuote,
  messagesReference,
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
import { EMPTY_SELECTION, orderedSelection, reduceMessageSelection, selectionActive } from "./messageSelection";

export interface SelectableRow {
  msg: NormalizedChatMessage;
  item: TimelineItem;
}

const INTERACTIVE = "a, button, input, textarea, select, summary, label, [role='button'], [role='checkbox'], [contenteditable='true']";

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

/** Quote: the messages as attributed blockquotes in one numbered quote capsule. */
export function quoteMessages(tabId: string, clips: readonly ClipMessage[]) {
  if (clips.length) insertIntoComposer(tabId, { kind: "quote", detail: messagesReference(clips), content: messagesQuote(clips) });
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

/** Selection of whole chat rows: Shift+click or the row checkbox, arrows, Space, Esc, and the bar's actions. */
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

  useEffect(() => { dispatch({ type: "prune", order }); }, [order]);

  const clipsOf = useCallback((ids: readonly string[]): ClipMessage[] => {
    const byId = new Map(live.current.rows.map((row) => [row.msg.id, row]));
    return ids.flatMap((id) => {
      const row = byId.get(id);
      return row ? [rowClip(row, live.current.names)] : [];
    });
  }, []);
  const clipFor = useCallback((id: string): ClipMessage | null => clipsOf([id])[0] ?? null, [clipsOf]);
  const selectedClips = useCallback(() => clipsOf(orderedSelection(live.current.state, live.current.order)), [clipsOf]);

  const toggle = useCallback((id: string) => dispatch({ type: "toggle", id }), []);
  const clear = useCallback(() => dispatch({ type: "clear" }), []);
  const copySelection = useCallback(() => writeClipboardFlavours(messagesClipboard(selectedClips())), [selectedClips]);
  const quoteSelection = useCallback(() => { quoteMessages(tabId, selectedClips()); dispatch({ type: "clear" }); }, [selectedClips, tabId]);
  const askSelection = useCallback(() => { askAboutMessages(tabId, selectedClips()); dispatch({ type: "clear" }); }, [selectedClips, tabId]);

  const rowMouseDown = useCallback((id: string, event: MouseEvent<HTMLElement>) => {
    if (!event.shiftKey || event.button !== 0 || nativeTextSelected()) return;
    if ((event.target as HTMLElement).closest(INTERACTIVE)) return;
    event.preventDefault();
    dispatch(selectionActive(live.current.state) ? { type: "extend", id, order: live.current.order } : { type: "toggle", id });
  }, []);

  const rowClick = useCallback((id: string, event: MouseEvent<HTMLElement>) => {
    if (event.shiftKey || !selectionActive(live.current.state) || nativeTextSelected()) return;
    if ((event.target as HTMLElement).closest(INTERACTIVE)) return;
    dispatch({ type: "toggle", id });
  }, []);

  const onKeyDown = useCallback((event: KeyboardEvent<HTMLElement>) => {
    const target = event.target as HTMLElement;
    const row = target.closest<HTMLElement>("[data-message-id]");
    const onRow = row !== null && (target === row || target.hasAttribute("data-message-select"));
    if (!onRow || event.altKey || event.ctrlKey || event.metaKey) return;
    const id = row.dataset.messageId ?? "";
    if (event.code === "ArrowDown" || event.code === "ArrowUp") {
      event.preventDefault();
      if (live.current.state.cursor !== id) dispatch({ type: "focus", id });
      dispatch({ type: "move", delta: event.code === "ArrowDown" ? 1 : -1, order: live.current.order, extend: event.shiftKey });
      focusCursor.current = true;
    } else if (event.code === "Space" && target === row) {
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
  }, [state.cursor, surfaceRef]);

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
    clipFor, toggle, rowMouseDown, rowClick, quote: (clip) => quoteMessages(tabId, [clip]),
  }), [clipFor, rowClick, rowMouseDown, tabId, toggle]);
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
