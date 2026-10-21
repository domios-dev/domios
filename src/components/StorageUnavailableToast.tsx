import { createNoticePublisher } from "../lib/notices";
import { useEffect, useRef } from "react";
import { useMultiplexer } from "../store/multiplexer";

const TOAST_ID = "storage-unavailable";

export function StorageUnavailableToast({ active }: { active: boolean }) {
  const { dismissToast, upsertToast } = useMultiplexer();
  const wasActive = useRef(false);

  useEffect(() => {
    if (!active) {
      if (wasActive.current) dismissToast(TOAST_ID);
      wasActive.current = false;
      return;
    }
    wasActive.current = true;
    createNoticePublisher({ upsertToast })({ kind: "toast", id: TOAST_ID,
      title: "Storage is unavailable",
      detail: "Your data is preserved. See the incident.",
      severity: "Error",
      tabId: null,
      workspaceId: null });
  }, [active, dismissToast, upsertToast]);

  return null;
}
