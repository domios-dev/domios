import { useMemo, useSyncExternalStore } from "react";
import type { ChatBlockHost, LiveBindingSource } from "@codeterm/chat-ui";
import { planSidePanelStore } from "../Canvas/planSidePanelStore";
import { insertIntoComposer } from "./composerInsertion";

const STORAGE_KEY = "codeterm.pinnedChatBlocks.v1";
const EMPTY: readonly string[] = [];

/** Pins a block source to the end of the list, or unpins it when already pinned. */
export function togglePinned(pins: readonly string[], source: string): readonly string[] {
  return pins.includes(source) ? pins.filter((pin) => pin !== source) : [...pins, source];
}

function load(): Map<string, readonly string[]> {
  try {
    const raw = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "{}") as Record<string, unknown>;
    return new Map(Object.entries(raw).flatMap(([tabId, pins]) =>
      Array.isArray(pins) ? [[tabId, pins.filter((pin): pin is string => typeof pin === "string")] as const] : []));
  } catch {
    return new Map();
  }
}

let pinsByTab: Map<string, readonly string[]> | null = null;
const listeners = new Set<() => void>();

function pinsMap() {
  pinsByTab ??= load();
  return pinsByTab;
}

function save() {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(Object.fromEntries(pinsMap())));
  } catch {
    return;
  }
}

export const pinnedBlocksStore = {
  list(tabId: string): readonly string[] {
    return pinsMap().get(tabId) ?? EMPTY;
  },
  /** Returns whether the source is pinned afterwards. */
  toggle(tabId: string, source: string): boolean {
    const next = togglePinned(this.list(tabId), source);
    if (next.length) pinsMap().set(tabId, next);
    else pinsMap().delete(tabId);
    save();
    for (const listener of listeners) listener();
    return next.includes(source);
  },
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  },
  resetForTest() {
    pinsByTab = new Map();
    for (const listener of listeners) listener();
  },
};

export function usePinnedBlocks(tabId: string): readonly string[] {
  return useSyncExternalStore(pinnedBlocksStore.subscribe, () => pinnedBlocksStore.list(tabId), () => EMPTY);
}

/** Tab-scoped block services: Ask goes to this tab's composer, Pin keeps the block live in this tab's side panel. */
export function useTabChatBlockHost(tabId: string): ChatBlockHost {
  return useMemo(() => {
    const pinned: LiveBindingSource<boolean> = {
      subscribe: pinnedBlocksStore.subscribe,
      read: (source) => pinnedBlocksStore.list(tabId).includes(source),
      refresh: () => {},
    };
    return {
      askAbout: ({ title, text }) => insertIntoComposer(tabId, { kind: "quote", detail: title, content: text }),
      pin: (source) => {
        if (pinnedBlocksStore.toggle(tabId, source)) planSidePanelStore.open({ planId: "", documentsTabId: tabId, tabView: "documents" });
      },
      pinned,
    };
  }, [tabId]);
}
