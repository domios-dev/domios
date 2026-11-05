import { useCallback, useMemo } from "react";
import { nativeBridge } from "../../lib/nativeBridge";
import { useAiHelperLaunch } from "../../lib/useAiHelperLaunch";
import type { DaemonApiConfig } from "../../lib/webApi";
import type { ServeStatus } from "../../lib/webAccess";
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
  const { launching, launch } = useAiHelperLaunch("mesh-tailscale-setup-failure", onOpened);
  const onSetup = useCallback((machine: string | null) => launch(() => startTailscaleSetup(machine, api)), [api, launch]);
  const onInstall = useCallback(() => { void nativeBridge.openUrl(TAILSCALE_DOWNLOAD_URL); }, []);
  const state = useMemo(() => tailscaleState({ serve, network, loading }), [serve, network, loading]);
  return useMemo(() => ({ state, identity: network?.transport?.identity, launching, onSetup, onInstall }), [state, network?.transport?.identity, launching, onSetup, onInstall]);
}
