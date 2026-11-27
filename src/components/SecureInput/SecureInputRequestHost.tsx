import { useCallback, useEffect, useState } from "react";
import { listen } from "../../lib/bridge";
import { selectedDaemonTransport } from "../../lib/daemonRuntime";
import { EVENT_STREAM_READY } from "../../lib/eventStreamReady";
import {
  SecureInputRequestCard,
  type SecureInputRequestView,
} from "./SecureInputRequestCard";

const CHANGED_EVENT = "secure-input-requests-changed";

export function SecureInputRequestHost() {
  const [requests, setRequests] = useState<SecureInputRequestView[]>([]);
  const [dismissedIds, setDismissedIds] = useState<Set<string>>(() => new Set());

  const refresh = useCallback(async () => {
    try {
      const data = await selectedDaemonTransport.localJson<{
        requests?: SecureInputRequestView[];
      }>("GET", "/api/secure-input/requests");
      if (Array.isArray(data?.requests)) {
        setRequests(data.requests);
      }
    } catch {
      // Daemon not yet ready or caller is non-owner; stay silent until ready.
    }
  }, []);

  useEffect(() => {
    let active = true;
    void refresh();
    let unlisten: (() => void) | null = null;
    // The event carries no metadata (P1-6); always fetch through the owner-only REST route.
    void listen<Record<string, never>>(CHANGED_EVENT, () => {
      if (active) void refresh();
    }).then((fn) => {
      if (active) unlisten = fn;
      else fn();
    });
    const onReconnect = () => {
      if (active) void refresh();
    };
    window.addEventListener(EVENT_STREAM_READY, onReconnect);
    return () => {
      active = false;
      unlisten?.();
      window.removeEventListener(EVENT_STREAM_READY, onReconnect);
    };
  }, [refresh]);

  const handleSubmit = useCallback(
    async (requestId: string, value: string, expectedInstallationId: string) => {
      const updated = await selectedDaemonTransport.localJson<SecureInputRequestView>(
        "POST",
        `/api/secure-input/requests/${encodeURIComponent(requestId)}/submit`,
        { value, expectedInstallationId },
      );
      setRequests((prev) =>
        prev.map((item) => (item.requestId === requestId ? updated : item)),
      );
      return updated;
    },
    [],
  );

  const handleRefuse = useCallback(async (requestId: string) => {
    const updated = await selectedDaemonTransport.localJson<SecureInputRequestView>(
      "POST",
      `/api/secure-input/requests/${encodeURIComponent(requestId)}/cancel`,
      { reason: "Refused by owner" },
    );
    setRequests((prev) =>
      prev.map((item) => (item.requestId === requestId ? updated : item)),
    );
  }, []);

  const handleDismiss = useCallback((requestId: string) => {
    setDismissedIds((prev) => {
      const next = new Set(prev);
      next.add(requestId);
      return next;
    });
  }, []);

  const visible = requests.filter(
    (req) =>
      req.status === "pending" ||
      req.status === "submitting" ||
      (!dismissedIds.has(req.requestId) &&
        (req.status === "delivered" || req.status === "uncertain")),
  );

  if (visible.length === 0) return null;

  return (
    <aside
      aria-label="Pending secure input requests"
      className="fixed bottom-4 right-4 z-50 flex w-full max-w-md flex-col gap-2.5 px-2"
    >
      {visible.map((req) => (
        <SecureInputRequestCard
          key={req.requestId}
          request={req}
          onSubmit={handleSubmit}
          onRefuse={handleRefuse}
          onDismiss={handleDismiss}
        />
      ))}
    </aside>
  );
}
