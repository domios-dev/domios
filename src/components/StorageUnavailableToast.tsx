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
    upsertToast({
      id: TOAST_ID,
      title: "Storage is unavailable",
      body: "Your data is preserved. See the incident.",
      severity: "Error",
      level: "failure",
      tab_id: null,
      workspace_id: null,
      sticky: true,
    });
  }, [active, dismissToast, upsertToast]);

  return null;
}
