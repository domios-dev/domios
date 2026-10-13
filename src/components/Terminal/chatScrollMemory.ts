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

export function chatScrollRestoreTarget(bookmark: ChatScrollBookmark, anchorTop: number | null, maximumTop: number): { mode: "pinned" | "detached"; top: number } {
  return bookmark.mode === "detached" && anchorTop != null && Number.isFinite(anchorTop)
    && anchorTop >= 0 && anchorTop <= maximumTop
    ? { mode: "detached", top: anchorTop }
    : { mode: "pinned", top: Math.max(0, maximumTop) };
}

export class ChatScrollMemory {
  private readonly states = new Map<string, ChatScrollState>();

  constructor(private readonly capacity = 32) {}

  read(key: string): ChatScrollState | null { return this.states.get(key) ?? null; }

  save(key: string, state: ChatScrollState): void {
    this.states.delete(key);
    this.states.set(key, { ...state, anchor: state.anchor ? { ...state.anchor } : null });
    while (this.states.size > this.capacity) {
      const oldest = this.states.keys().next().value;
      if (oldest == null) break;
      this.states.delete(oldest);
    }
  }

  clear(key: string): void { this.states.delete(key); }
}
