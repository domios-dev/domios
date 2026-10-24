import { useCallback, useMemo, useState } from "react";
import { nativeBridge } from "../../lib/nativeBridge";
import { createNoticePublisher } from "../../lib/notices";
import type { DaemonApiConfig } from "../../lib/webApi";
import type { ServeStatus } from "../../lib/webAccess";
import { useMultiplexer } from "../../store/multiplexer/context";
import type { CanvasTailscale } from "./TailscaleNodes";
import { startTailscaleSetup, TAILSCALE_DOWNLOAD_URL, tailscaleState } from "./tailscaleSetup";
import type { NetworkInfo } from "./webUiTarget";

/** Tailscale state for the canvas plus the General agent hand-off; `onOpened` runs once the agent tab is up. */
export function useCanvasTailscale({ serve, network, loading, api, onOpened }: {
  serve: ServeStatus | null;
  network: NetworkInfo | null;
  loading: boolean;
  api: DaemonApiConfig;
  onOpened: () => void;
}): CanvasTailscale {
  const { upsertToast } = useMultiplexer();
  const [launching, setLaunching] = useState(false);
  const onSetup = useCallback((machine: string | null) => {
    setLaunching(true);
    startTailscaleSetup(machine, api).then(onOpened, (error: unknown) => {
      createNoticePublisher({ upsertToast })({ kind: "toast", id: "mesh-tailscale-setup-failure",
        title: "The General agent could not be opened",
        detail: error instanceof Error ? error.message : String(error),
        severity: "Warning", tabId: null, workspaceId: null });
    }).finally(() => setLaunching(false));
  }, [api, onOpened, upsertToast]);
  const onInstall = useCallback(() => { void nativeBridge.openUrl(TAILSCALE_DOWNLOAD_URL); }, []);
  const state = useMemo(() => tailscaleState({ serve, network, loading }), [serve, network, loading]);
  return useMemo(() => ({ state, launching, onSetup, onInstall }), [state, launching, onSetup, onInstall]);
}
