import { useEffect, useState } from "react";
import { WifiOff } from "lucide-react";

export function OfflineIndicator() {
  const [offline, setOffline] = useState(() => !navigator.onLine);
  const [cacheHydrated, setCacheHydrated] = useState(false);

  useEffect(() => {
    const onOnline = () => setOffline(false);
    const onOffline = () => setOffline(true);
    const onHydrated = () => setCacheHydrated(true);
    const onReconciled = () => setCacheHydrated(false);

    window.addEventListener("online", onOnline);
    window.addEventListener("offline", onOffline);
    window.addEventListener("codeterm-cache-hydrated", onHydrated);
    window.addEventListener("codeterm-cache-reconciled", onReconciled);
    return () => {
      window.removeEventListener("online", onOnline);
      window.removeEventListener("offline", onOffline);
      window.removeEventListener("codeterm-cache-hydrated", onHydrated);
      window.removeEventListener("codeterm-cache-reconciled", onReconciled);
    };
  }, []);

  if (!offline) return null;

  return (
    <div
      data-testid="offline-indicator"
      role="status"
      className="fixed top-[calc(var(--ct-safe-top,0px)+0.5rem)] left-1/2 -translate-x-1/2 z-[9998] flex items-center gap-1.5 rounded-full border border-[var(--ct-border,#30363d)] bg-[var(--ct-titlebar-bg,#161b22)] px-3 py-1.5 text-[11px] text-[var(--ct-fg,#e6edf3)] shadow-lg pointer-events-none"
    >
      <WifiOff size={12} className="shrink-0 opacity-80" />
      <span>
        Offline
        {cacheHydrated ? " — showing last saved state" : ""}
      </span>
    </div>
  );
}
