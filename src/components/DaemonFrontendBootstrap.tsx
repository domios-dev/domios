import { frontendCheckpointClientId, frontendUpdateDiagnostic } from "../lib/frontendUpdate/bootstrapModel";
import { frontendApiContract } from "../lib/frontendApiContract";
import { frontendBootDiagnostic } from "../lib/frontendBootDiagnostic";
import { useEffect, useState, type ReactNode } from "react";
import { baseDaemonTransport, FRONTEND_BOOT_KEY, onDaemonTransport } from "../lib/daemonBootstrap";
import { initializeBrowserFrontendUpdates } from "../lib/frontendUpdate/browser";
import type { BootContext } from "../lib/frontendUpdate/client";
import { getResizeClientId } from "../lib/resizeOwner";
import { waitForServerReady, type StartupPhase } from "../lib/serverReadiness";

const startupPhaseLabels: Record<StartupPhase, string> = {
  listener: "Opening API listener",
  integrity: "Verifying resources",
  states: "Initializing state",
  providers: "Registering providers",
  service: "Preparing service",
  plans: "Loading plans",
  background: "Starting background services",
  platform: "Preparing platform",
};

export function DaemonFrontendBootstrap({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(false);
  const [startupFailed, setStartupFailed] = useState(false);
  const [startupPhase, setStartupPhase] = useState<StartupPhase | null>(null);
  useEffect(() => {
    let generation = 0;
    let dispose: (() => void) | undefined;
    let initializing = Promise.resolve();
    let startupWait: AbortController | undefined;
    const stop = onDaemonTransport(client => {
      const current = ++generation;
      startupWait?.abort();
      const controller = new AbortController();
      startupWait = controller;
      setReady(false);
      setStartupFailed(false);
      setStartupPhase(null);
      initializing = initializing.then(async () => {
        if (current !== generation) return;
        dispose?.();
        dispose = undefined;
        try {
          await waitForServerReady(client, controller.signal, phase => {
            if (current === generation) setStartupPhase(phase);
          });
        } catch (failure) {
          if (current === generation) {
            frontendBootDiagnostic("frontend-initialize-failed", frontendUpdateDiagnostic(failure));
            setStartupFailed(true);
          }
          return;
        }
        if (current !== generation) return;
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
    return () => { ++generation; startupWait?.abort(); stop(); dispose?.(); };
  }, []);
  if (startupFailed) return <div role="alert">Domios could not finish starting.</div>;
  return ready ? children : <div role="status" aria-live="polite">Starting Domios…{startupPhase && ` ${startupPhaseLabels[startupPhase]}`}</div>;
}
