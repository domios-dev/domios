import { useEffect, useState, type RefObject } from "react";

export function hasPaintedContent(root: ParentNode): boolean {
  if (root.querySelector("[data-chat-window-row]")) return true;
  const rows = root.querySelector(".xterm-rows");
  return Boolean(rows?.textContent?.trim());
}

/** True once the tab body has shown real content, so a later restore can stay non-blocking. */
export function useBodyPainted(ref: RefObject<HTMLElement | null>, tabId: string): boolean {
  const [painted, setPainted] = useState(false);
  useEffect(() => {
    setPainted(false);
    const root = ref.current;
    if (!root) return;
    if (hasPaintedContent(root)) {
      setPainted(true);
      return;
    }
    if (typeof MutationObserver === "undefined") return;
    const observer = new MutationObserver(() => {
      if (!hasPaintedContent(root)) return;
      setPainted(true);
      observer.disconnect();
    });
    observer.observe(root, { childList: true, subtree: true, characterData: true });
    return () => observer.disconnect();
  }, [ref, tabId]);
  return painted;
}
