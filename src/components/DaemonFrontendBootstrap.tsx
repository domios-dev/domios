import { frontendCheckpointClientId, frontendUpdateDiagnostic } from "../lib/frontendUpdate/bootstrapModel";
import { frontendApiContract } from "../lib/frontendApiContract";
import { frontendBootDiagnostic } from "../lib/frontendBootDiagnostic";
import { useEffect, useState, type ReactNode } from "react";
import { baseDaemonTransport, FRONTEND_BOOT_KEY, onDaemonTransport } from "../lib/daemonBootstrap";
import { initializeBrowserFrontendUpdates } from "../lib/frontendUpdate/browser";
import type { BootContext } from "../lib/frontendUpdate/client";
import { getResizeClientId } from "../lib/resizeOwner";

export function DaemonFrontendBootstrap({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(false);
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
        let failed = false;
        const disableUpdates = (failure: unknown) => {
          frontendBootDiagnostic("frontend-initialize-failed", frontendUpdateDiagnostic(failure));
          if (current !== generation) return;
          failed = true;
          dispose?.();
          dispose = undefined;
          setReady(true);
        };
        try {
          const stored = sessionStorage.getItem(FRONTEND_BOOT_KEY);
          const boot = stored ? JSON.parse(stored) as BootContext : null;
          frontendBootDiagnostic("frontend-initialize");
          const initialized = await initializeBrowserFrontendUpdates(client, {
            clientId: boot?.clientId ?? frontendCheckpointClientId(getResizeClientId()), boot,
            bundled: boot?.bundled ?? { index: "/bundled/index.html", minApiVersion: frontendApiContract.minApiVersion, maxApiVersion: frontendApiContract.maxApiVersion },
            onError: disableUpdates,
          });
          if (current !== generation || failed) { initialized.dispose(); return; }
          dispose = initialized.dispose;
          frontendBootDiagnostic("frontend-initialized");
          setReady(true);
        } catch (failure) { disableUpdates(failure); }
      });
    });
    baseDaemonTransport();
    return () => { ++generation; stop(); dispose?.(); };
  }, []);
  return ready ? children : null;
}
