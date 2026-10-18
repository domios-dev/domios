import type { ChatContentAnchor } from "./chatAnchorModel";

export interface ChatScrollState {
  anchor?: ChatContentAnchor | null;
  mode: "pinned" | "detached";
  scrollTop: number;
}

export type ChatScrollBookmark = { mode: "pinned" } | { mode: "detached"; anchor: ChatContentAnchor };

export function chatScrollBookmark(state: ChatScrollState | null): ChatScrollBookmark {
  return state?.mode === "detached" && state.anchor
    ? { mode: "detached", anchor: { key: state.anchor.key, ordinal: state.anchor.ordinal, offset: state.anchor.offset } }
    : { mode: "pinned" };
}

export function initialChatScrollAnchor(state: ChatScrollState | null): {
  pinned: boolean; top: number; anchor: ChatContentAnchor | null;
} {
  const bookmark = chatScrollBookmark(state);
  return bookmark.mode === "detached"
    ? { pinned: false, top: state!.scrollTop, anchor: bookmark.anchor }
    : { pinned: true, top: 0, anchor: null };
}

export function chatScrollRestoreTarget(bookmark: ChatScrollBookmark, anchorTop: number | null, maximumTop: number): { mode: "pinned" | "detached"; top: number } {
  return bookmark.mode === "detached" && anchorTop != null && Number.isFinite(anchorTop)
    && anchorTop >= 0 && anchorTop <= maximumTop
    ? { mode: "detached", top: anchorTop }
    : { mode: "pinned", top: Math.max(0, maximumTop) };
}

export class ChatScrollMemory {
  private readonly states = new Map<string, ChatScrollState>();

  constructor(private readonly capacity = 32, private readonly persistence?: {
    storage: Pick<Storage, "getItem" | "setItem">; key: string;
  }) {
    if (!persistence) return;
    try {
      for (const [key, state] of decodeChatScrollMemory(persistence.storage.getItem(persistence.key))) {
        this.states.set(key, state);
      }
      this.trim();
    } catch {}
  }

  read(key: string): ChatScrollState | null { return this.states.get(key) ?? null; }

  save(key: string, state: ChatScrollState): void {
    this.states.delete(key);
    this.states.set(key, { ...state, anchor: state.anchor ? { ...state.anchor } : null });
    this.trim();
    this.persist();
  }

  private trim(): void {
    while (this.states.size > this.capacity) {
      const oldest = this.states.keys().next().value;
      if (oldest == null) break;
      this.states.delete(oldest);
    }
  }

  private persist(): void {
    try { this.persistence?.storage.setItem(this.persistence.key, JSON.stringify([...this.states])); } catch {}
  }

  clear(key: string): void { this.states.delete(key); this.persist(); }
}

export function decodeChatScrollMemory(raw: string | null): Array<[string, ChatScrollState]> {
  if (!raw) return [];
  let entries: unknown;
  try { entries = JSON.parse(raw); } catch { return []; }
  if (!Array.isArray(entries)) return [];
  return entries.filter((entry): entry is [string, ChatScrollState] => {
    if (!Array.isArray(entry) || entry.length !== 2 || typeof entry[0] !== "string") return false;
    const state = entry[1];
    if (!state || (state.mode !== "pinned" && state.mode !== "detached")
      || !Number.isFinite(state.scrollTop) || state.scrollTop < 0) return false;
    const anchor = state.anchor;
    return anchor == null || (typeof anchor.key === "string" && Number.isInteger(anchor.ordinal) && anchor.ordinal >= 0
      && Number.isFinite(anchor.offset) && (anchor.id == null || typeof anchor.id === "string"));
  });
}
