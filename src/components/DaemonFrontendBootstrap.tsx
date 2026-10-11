import { useEffect, useState, type ReactNode } from "react";
import { baseDaemonTransport, FRONTEND_BOOT_KEY, onDaemonTransport } from "../lib/daemonBootstrap";
import { initializeBrowserFrontendUpdates } from "../lib/frontendUpdate/browser";
import type { BootContext } from "../lib/frontendUpdate/client";
import { getResizeClientId } from "../lib/resizeOwner";

export function DaemonFrontendBootstrap({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<unknown>();
  useEffect(() => {
    let generation = 0;
    let dispose: (() => void) | undefined;
    let initializing = Promise.resolve();
    const stop = onDaemonTransport(client => {
      const current = ++generation;
      setReady(false);
      initializing = initializing.then(async () => {
        if (current !== generation) return;
        dispose?.();
        dispose = undefined;
        try {
          const stored = sessionStorage.getItem(FRONTEND_BOOT_KEY);
          const boot = stored ? JSON.parse(stored) as BootContext : null;
          const initialized = await initializeBrowserFrontendUpdates(client, {
            clientId: boot?.clientId ?? getResizeClientId(), boot,
            bundled: boot?.bundled ?? { index: "/bundled/index.html", minApiVersion: 2, maxApiVersion: 2 },
            onError: failure => { if (current === generation) setError(failure); },
          });
          if (current !== generation) { initialized.dispose(); return; }
          dispose = initialized.dispose;
          setReady(true);
        } catch (failure) { if (current === generation) setError(failure); }
      });
    });
    baseDaemonTransport();
    return () => { ++generation; stop(); dispose?.(); };
  }, []);
  if (error) throw error;
  return ready ? children : null;
}
