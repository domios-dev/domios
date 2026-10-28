import { useEffect, useRef, type RefObject } from "react";
import type { HistoryEntry } from "./neighborConciergeClient";

const MESSAGE_ID = /^[0-9a-f]{32}$/;

/** Reports read up to the newest message from them that is on screen while the window is visible.
 *  Loading history proves nothing; without a way to observe visibility, nothing is reported. */
export function useNeighborReadReceipt(scroller: RefObject<HTMLElement | null>, entries: readonly HistoryEntry[] | null, friendship: string,
  markRead: (friendship: string, throughMessageId: string) => Promise<void>) {
  const reported = useRef<number>(-1);
  useEffect(() => { reported.current = -1; }, [friendship]);
  useEffect(() => {
    const root = scroller.current;
    if (!root || !entries || typeof IntersectionObserver === "undefined") return;
    const order = new Map(entries.filter(entry => entry.direction === "in" && MESSAGE_ID.test(entry.message_id)).map((entry, index) => [entry.message_id, index] as const));
    if (order.size === 0) return;
    const visible = new Set<string>();
    const report = () => {
      if (document.visibilityState !== "visible") return;
      let newest: string | null = null;
      for (const id of visible) if (newest === null || order.get(id)! > order.get(newest)!) newest = id;
      if (newest === null) return;
      const at = entries.find(entry => entry.message_id === newest)!.at;
      if (at <= reported.current) return;
      reported.current = at;
      void markRead(friendship, newest).catch(() => { if (reported.current === at) reported.current = -1; });
    };
    const observer = new IntersectionObserver(changes => {
      for (const change of changes) {
        const id = (change.target as HTMLElement).dataset.messageId;
        if (!id || !order.has(id)) continue;
        if (change.isIntersecting) visible.add(id);
        else visible.delete(id);
      }
      report();
    }, { root, threshold: 0.5 });
    for (const element of root.querySelectorAll<HTMLElement>('.neighbor-msg[data-side="in"][data-message-id]')) observer.observe(element);
    document.addEventListener("visibilitychange", report);
    return () => { observer.disconnect(); document.removeEventListener("visibilitychange", report); };
  }, [scroller, entries, friendship, markRead]);
}
