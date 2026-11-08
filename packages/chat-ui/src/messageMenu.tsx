import { createContext, useContext, useLayoutEffect, type ReactNode } from "react";
import type { ChatCardMenuItem, MessageMenuKind } from "./blocks/ChatBlockHost";

/**
 * One menu per chat row. A tool card's own items (Copy output, Details, Open full screen…) and its message's
 * items (Copy, Ask about this, Select…) merge into one list. On desktop the message's side-dot menu shows it
 * and the card draws no ⋮. On a compact touch surface the card's ⋮ shows it and the row draws no dot.
 */

type Group = "view" | "copy" | "ask" | "act";
const GROUP: Record<MessageMenuKind, Group> = {
  open: "view", copy: "copy", "copy-markdown": "copy", ask: "ask", details: "ask", select: "act", fork: "act", revert: "act", delete: "act",
};
const GROUPS: Group[] = ["view", "copy", "ask", "act"];
const ORDER: MessageMenuKind[] = ["open", "copy", "copy-markdown", "ask", "details", "select", "fork", "revert", "delete"];

export function messageMenuGroup(item: ChatCardMenuItem): Group {
  return item.kind ? GROUP[item.kind] : "copy";
}

/**
 * Merges a card's items into its message's items. The card's version wins where both offer the same action:
 * its output is the card's Copy, and its Ask carries the output. Items without a kind are the card's own
 * (Copy command, View changes…) and sit with the copy actions.
 */
export function mergeMessageMenu(message: readonly ChatCardMenuItem[], card: readonly ChatCardMenuItem[]): ChatCardMenuItem[] {
  const cardKinds = new Set(card.map((item) => item.kind).filter(Boolean));
  const rank = (item: ChatCardMenuItem) => GROUPS.indexOf(messageMenuGroup(item)) * 100 + (item.kind ? ORDER.indexOf(item.kind) : 1.5);
  const merged = [
    ...card.map((item) => item.kind === "copy" ? { ...item, label: "Copy" } : item),
    ...message.filter((item) => !item.kind || !cardKinds.has(item.kind)),
  ];
  return merged.map((item, index) => ({ item, index })).sort((a, b) => rank(a.item) - rank(b.item) || a.index - b.index).map(({ item }) => item);
}

export interface MessageMenuBridge {
  /** The message row's own items, built when a menu draws them. */
  messageItems(): ChatCardMenuItem[];
  /** The card that speaks for the row; the first card to claim it wins. */
  owner: string | null;
  claim(key: string): void;
  release(key: string): void;
  /** The owner's current items, read by the row's menu when it opens. */
  publish(key: string, items: readonly ChatCardMenuItem[]): void;
}

const MessageMenuBridgeContext = createContext<MessageMenuBridge | null>(null);

export const MessageMenuBridgeProvider = MessageMenuBridgeContext.Provider;

/** Cards inside this (a tool stream's members, one of several diffs) keep their own ⋮ instead of speaking for the row. */
export function RowMenuIsolate({ children }: { children: ReactNode }) {
  return <MessageMenuBridgeContext.Provider value={null}>{children}</MessageMenuBridgeContext.Provider>;
}

/** Joins a card's menu to its row: returns the bridge while this card owns the row's menu, otherwise null. */
export function useRowMenu(key: string, items: readonly ChatCardMenuItem[], enabled: boolean): MessageMenuBridge | null {
  const bridge = useContext(MessageMenuBridgeContext);
  const active = enabled ? bridge : null;
  const claim = active?.claim;
  const release = active?.release;
  const free = active?.owner === null;
  useLayoutEffect(() => {
    if (!claim || !release || !free) return;
    claim(key);
  }, [claim, release, free, key]);
  useLayoutEffect(() => () => release?.(key), [release, key]);
  useLayoutEffect(() => {
    if (active?.owner === key) active.publish(key, items);
  });
  return active?.owner === key ? active : null;
}
