import { useLayoutEffect, useState, type RefObject } from "react";

export function hasPaintedContent(root: ParentNode): boolean {
  const terminal = root.querySelector<HTMLElement>('[data-terminal-visible="true"]');
  if (terminal?.style.visibility === "visible" && terminal.querySelector(".xterm-rows")?.textContent?.trim()) return true;
  const chat = root.querySelector("[data-chat-ready]");
  if (chat) return chat.getAttribute("data-chat-ready") === "true";
  if (root.querySelector("[data-chat-window-row]")) return true;
  const rows = root.querySelector(".xterm-rows");
  return Boolean(rows?.textContent?.trim());
}

/** Once real content is ready, later restore work stays non-blocking for this tab. */
export function useBodyPainted(ref: RefObject<HTMLElement | null>, tabId: string, canReveal = true): boolean {
  const [painted, setPainted] = useState({ tabId, ready: false });
  const ready = painted.tabId === tabId && painted.ready;
  if (painted.tabId !== tabId) setPainted({ tabId, ready: false });
  useLayoutEffect(() => {
    if (ready || !canReveal) return;
    const root = ref.current;
    if (!root) return;
    if (hasPaintedContent(root)) {
      setPainted({ tabId, ready: true });
      return;
    }
    if (typeof MutationObserver === "undefined") return;
    const observer = new MutationObserver(() => {
      if (!hasPaintedContent(root)) return;
      setPainted({ tabId, ready: true });
      observer.disconnect();
    });
    observer.observe(root, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ["data-chat-ready"] });
    return () => observer.disconnect();
  }, [ref, tabId, canReveal, ready]);
  return ready;
}
