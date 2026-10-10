import { useEffect, useRef, useState } from "react";
import type { Notification, Tab, TabStatus } from "../../lib/types";
import { tabAttention } from "../Sidebar/tabAttention";

export function useTabAttention(tab: Tab, status: TabStatus | null | undefined, notifications: readonly Notification[], active: boolean, markRead: (id: string) => void) {
  const [visitedId, setVisitedId] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState<string | null>(null);
  const marked = useRef(new Set<string>());
  const pending = tabAttention(tab, status, notifications, active);
  const visited = notifications.find((item) => item.id === visitedId && !item.resolved);
  const model = pending.notification || !visited
    ? pending
    : tabAttention(tab, status, [{ ...visited, read: false }], active);
  const key = model.notification?.id ?? `waiting:${model.reason}`;

  useEffect(() => {
    if (!active) return;
    const attention = tabAttention(tab, status, notifications, true);
    if (attention.notification) setVisitedId(attention.notification.id);
    for (const id of attention.notificationIds) {
      if (marked.current.has(id)) continue;
      marked.current.add(id);
      markRead(id);
    }
  }, [active, tab, status, notifications, markRead]);

  return { model: active && model.wantsAttention && dismissed !== key ? model : null, dismiss: () => setDismissed(key) };
}
