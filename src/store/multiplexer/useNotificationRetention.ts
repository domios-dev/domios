import { useEffect, type Dispatch } from "react";
import { BELL_NOTIFICATION_AGE_MS } from "../../lib/notificationAdmission";
import type { Notification } from "../../lib/notificationTypes";
import type { Action } from "./reducer";

export function useNotificationRetention(notifications: Notification[], dispatch: Dispatch<Action>) {
  const earliest = notifications.reduce((timestamp, row) => Math.min(timestamp, row.timestamp), Infinity);
  useEffect(() => {
    if (!Number.isFinite(earliest)) return;
    const deadline = earliest + BELL_NOTIFICATION_AGE_MS;
    let timer: ReturnType<typeof setTimeout>;
    const pruneAtDeadline = () => {
      const now = Date.now();
      if (now < deadline) {
        timer = setTimeout(pruneAtDeadline, Math.min(deadline - now, 2 ** 31 - 1));
      } else {
        dispatch({ type: "PRUNE_NOTIFICATIONS", now });
      }
    };
    timer = setTimeout(pruneAtDeadline, Math.min(Math.max(0, deadline - Date.now()), 2 ** 31 - 1));
    return () => clearTimeout(timer);
  }, [dispatch, earliest]);
}
