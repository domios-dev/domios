import type { Notification } from "../../lib/types";
import { createRefreshCoalescer } from "./refreshCoalescer";

export function mountNotificationHistory(
  read: () => Promise<Notification[]>,
  apply: (notifications: Notification[]) => void,
  subscribe: (refresh: () => void) => () => void,
  failed: (error: unknown) => void,
): () => void {
  let active = true;
  const refresh = createRefreshCoalescer(async () => {
    if (!active) return false;
    try {
      const notifications = await read();
      if (active) apply(notifications);
      return true;
    } catch (error) {
      if (active) failed(error);
      return false;
    }
  });
  const request = () => { if (active) void refresh.request(); };
  const unsubscribe = subscribe(request);
  request();
  return () => {
    active = false;
    unsubscribe();
  };
}
