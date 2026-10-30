import { nativeBridge } from "../../lib/nativeBridge";
import { useEffect, useLayoutEffect, useMemo, useRef, useState, useCallback } from "react";
import { RefreshCw } from "lucide-react";
import { invoke, listen } from "../../lib/bridge";
import { BusySpinner, BusyState } from "../shared/BusyState";
import type { ApiConfig, AppConfig } from "../../lib/types";
import type { MeshTab } from "./types";
import type { RecentPush } from "./MeshCanvas";
import { AccountMeshCanvas } from "./AccountMeshCanvas";
import { useCanvasTailscale } from "./useCanvasTailscale";
import { ServiceDrawer, type DrawerState } from "./ServiceDrawer";
import { useMeshData } from "./useMeshData";
import { meshDataUnavailable, meshHeaderState } from "./meshProjection";
import { DeferredLoader } from "@codeterm/chat-ui/DeferredLoader";
import { Loader } from "../shared/Loader";
import { NetworkTab } from "./NetworkTab";
import { ActivityTab } from "./ActivityTab";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { deriveWebUiView } from "./meshWebUi";
import { WebNodeCard } from "./WebNodeCard";
import { generateWebQr, type WebUiAction } from "./WebAccessPanel";
import { UI_PANEL, UI_TEXT } from "../shared/design";
import { LiquidTabRail } from "../shared/LiquidTabRail";
import {
  startWebAccessSetupHelper,
  type ServeStatus,
} from "../../lib/webAccess";
import { buildWebUiTarget, displayedWebUrl, type NetworkInfo } from "./webUiTarget";

const WEB_ACTIONS = {
  open: (link: string) => { void nativeBridge.openUrl(link); },
  copy: (link: string) => navigator.clipboard.writeText(link),
  qr: generateWebQr,
};

const openPeerWebUi = (url: string) => { void nativeBridge.openUrl(url); };

const TABS: MeshTab[] = ["inventory", "activity", "network"];

interface ServeResult {
  ok: boolean;
  code: string;
  https_url?: string | null;
  warning?: string | null;
}

interface Props {
  api: ApiConfig;
  config: AppConfig;
  onConfigUpdate: (config: AppConfig) => Promise<void>;
  onClose: () => void;
}

function normalizeTab(t: unknown): MeshTab {
  if (t === "settings") return "network";
  if (t === "map") return "inventory";
  if (t === "inventory" || t === "activity" || t === "network") return t;
  return "inventory";
}

export function MeshPanel({ config, onConfigUpdate, onClose }: Props) {
  const touchUi = useTouchOptimizedUi();
  const goOnline = useCallback(async () => {
    await onConfigUpdate({ ...config, mesh: { ...config.mesh, enabled: true } });
  }, [config, onConfigUpdate]);
  const [activeTab, setActiveTab] = useState<MeshTab>(() => normalizeTab("map"));
  const [refreshKey, setRefreshKey] = useState(0);
  const [registryRefreshKey, setRegistryRefreshKey] = useState(0);
  const data = useMeshData(refreshKey);
  const [recentPushes, setRecentPushes] = useState<RecentPush[]>([]);
  const [network, setNetwork] = useState<NetworkInfo | null>(null);
  const [serve, setServe] = useState<ServeStatus | null>(null);
  const [serveLoading, setServeLoading] = useState(true);

  const triggerRefresh = useCallback(() => setRefreshKey((k) => k + 1), []);
  const refreshAll = useCallback(() => {
    triggerRefresh();
    setRegistryRefreshKey((key) => key + 1);
  }, [triggerRefresh]);
  const header = useMemo(
    () => meshHeaderState({ error: data.error, loaded: data.loaded, peers: data.peers }, activeTab === "inventory", config.mesh.enabled),
    [data.error, data.loaded, data.peers, activeTab, config.mesh.enabled],
  );
  const meshOff = !config.mesh.enabled;
  const unavailable = !meshOff && meshDataUnavailable(data);
  const graphReady = data.loaded;

  useEffect(() => {
    let unlistenHealth: (() => void) | null = null;
    let unlistenPush: (() => void) | null = null;
    let active = true;
    listen("service-health-changed", triggerRefresh).then((unlisten) => {
      if (active) unlistenHealth = unlisten;
      else unlisten();
    });
    listen<{ source_id: string; target: string }>("service-pushed", (event) => {
      if (!active) return;
      setRecentPushes((current) => [...current, { ...event.payload, at: Date.now() }].slice(-20));
    }).then((unlisten) => {
      if (active) unlistenPush = unlisten;
      else unlisten();
    });
    return () => {
      active = false;
      unlistenHealth?.();
      unlistenPush?.();
    };
  }, [triggerRefresh]);

  const loadServe = useCallback(async (accept: () => boolean = () => true) => {
    const [status, networkInfo] = await Promise.all([
      invoke<ServeStatus>("get_serve_status").catch(() => null),
      invoke<NetworkInfo>("get_network_info").catch(() => null),
    ]);
    if (accept()) {
      setNetwork(networkInfo);
      setServe(status);
      setServeLoading(false);
    }
    return status;
  }, []);

  useEffect(() => {
    let active = true;
    const refresh = () => {
      void loadServe(() => active);
    };
    refresh();
    let stop: (() => void) | undefined;
    void listen("mesh-state-changed", refresh).then((unlisten) => {
      if (active) stop = unlisten; else unlisten();
    });
    window.addEventListener("focus", refresh);
    return () => { active = false; stop?.(); window.removeEventListener("focus", refresh); };
  }, [loadServe, refreshKey]);

  const runWebUiAction = useCallback(
    async (action: WebUiAction): Promise<string | void> => {
      if (action === "disable") {
        const res = await invoke<ServeResult>("disable_web_access", {});
        await loadServe();
        if (!res.ok) throw new Error(`Could not disable (${res.code}).`);
        return "Web UI disabled.";
      }
      const snap = serve?.snapshot;
      if (action === "setup" && snap?.operator_bootstrap_required) {
        return snap.bootstrap_command
          ? `Run in a terminal: ${snap.bootstrap_command}, then Repair.`
          : "Grant Tailscale Serve operator access, then Repair.";
      }
      const res = await invoke<ServeResult>("enable_web_access", {});
      await loadServe();
      if (!res.ok) throw new Error(`Could not ${action} (${res.code}).`);
      return action === "repair" ? "Route reconciled." : res.warning ?? "Web UI enabled.";
    },
    [serve, loadServe],
  );

  const [drawerState, setDrawerState] = useState<DrawerState>(null);
  const [meshAnnouncement, setMeshAnnouncement] = useState("");
  const mapRef = useRef<HTMLDivElement>(null);
  const [mapSize, setMapSize] = useState({ width: 800, height: 600 });
  const handlePeerClick = useCallback((name: string) => {
    setDrawerState({ kind: "peer", id: name });
  }, []);
  const closeDrawer = useCallback(() => setDrawerState(null), []);
  const setupWebAccess = useCallback(async () => {
    await startWebAccessSetupHelper(serve, buildWebUiTarget(config, network, serve).port, config.api);
    onClose();
  }, [serve, config, network, onClose]);
  // The canvas and its QR card depend only on mesh data, so opening a drawer leaves them untouched.
  const webCard = useCallback((qrSize: number) => <WebNodeCard qrSize={qrSize} config={config} network={network} serve={serve} loading={serveLoading}
    actions={WEB_ACTIONS} onAction={runWebUiAction} onSetup={setupWebAccess} />, [config, network, serve, serveLoading, runWebUiAction, setupWebAccess]);
  const webUiTarget = useMemo(() => buildWebUiTarget(config, network, serve), [config, network, serve]);
  const tailscale = useCanvasTailscale({ serve, network, loading: serveLoading, api: config.api, onOpened: onClose });

  useLayoutEffect(() => {
    const element = mapRef.current;
    if (!element || activeTab !== "inventory") return;
    const measure = () => {
      const bounds = element.getBoundingClientRect();
      if (bounds.width <= 0 || bounds.height <= 0) return;
      setMapSize((current) => Math.abs(current.width - bounds.width) < 1 && Math.abs(current.height - bounds.height) < 1
        ? current
        : { width: bounds.width, height: bounds.height });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [activeTab, graphReady]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return;
      const inField =
        e.target instanceof HTMLInputElement ||
        e.target instanceof HTMLTextAreaElement;
      if (e.key === "Escape" && !inField) {
        e.preventDefault();
        onClose();
        return;
      }
      if (inField) return;
      if (e.key === "1") {
        e.preventDefault();
        setActiveTab("inventory");
      } else if (e.key === "2") {
        e.preventDefault();
        setActiveTab("activity");
      } else if (e.key === "3") {
        e.preventDefault();
        setActiveTab("network");
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  return (
    <div className="absolute inset-0 flex flex-col bg-[var(--ct-bg)] pt-[var(--ct-safe-top,0px)]">
      <div
        className={`${UI_PANEL.toolbar} ct-section-toolbar gap-0 px-4 py-0 text-[var(--ct-fg)] whitespace-nowrap`}
        style={touchUi ? { height: "auto", minHeight: "3.25rem" } : undefined}
      >
        <div className="ct-section-toolbar__identity flex items-center gap-3 mr-4 shrink-0">
          <strong>Mesh</strong>
          <div className={`${UI_TEXT.meta} opacity-70`}>
            {header.kind === "off"
              ? <span role="status">Mesh is off.</span>
              : header.kind === "unavailable"
              ? <span role="status">Mesh data is unavailable.</span>
              : header.kind === "loading"
                ? <DeferredLoader state="present" aria-busy="true"><BusyState inline label="Loading peers" phase="work" /></DeferredLoader>
                : `${header.count} peer${header.count === 1 ? "" : "s"}`}
          </div>
          {activeTab !== "inventory" && <button
            onClick={refreshAll}
            className={`flex ${touchUi ? "h-9 w-9" : "h-7 w-7"} shrink-0 items-center justify-center p-0 text-[var(--ct-muted-fg)] transition-colors hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] active:scale-95`}
            style={{ borderRadius: "9999px" }}
            title="Refresh"
          >
            <BusySpinner icon={RefreshCw} size={touchUi ? 16 : 13} spinning={data.loading} />
          </button>}
        </div>
        <div className="ct-section-toolbar__tabs border-l border-[var(--ct-border)] pl-3">
          <LiquidTabRail
            value={activeTab}
            options={TABS}
            onChange={setActiveTab}
            label="Mesh sections"
            touch={touchUi}
            testIdPrefix="mesh-tab"
          />
        </div>
      </div>
      <div className="relative flex-1 min-h-0 overflow-hidden">
        {activeTab === "inventory" && meshOff && (
          <section aria-label="Mesh inventory" className="absolute inset-0 flex flex-col items-center justify-center gap-3 text-[var(--ct-muted-fg)]">
            <p>Mesh is off.</p>
            <button
              type="button"
              onClick={() => { void goOnline(); }}
              className="flex items-center gap-2 rounded-md border border-[var(--ct-border)] px-3 py-1.5 text-[var(--ct-fg)] transition-colors hover:bg-[var(--ct-hover)]"
            >
              Turn on Mesh
            </button>
          </section>
        )}
        {activeTab === "inventory" && unavailable && (
          <section aria-label="Mesh inventory" className="absolute inset-0 flex flex-col items-center justify-center gap-3 text-[var(--ct-muted-fg)]">
            <p>Mesh data is unavailable.</p>
            <button
              type="button"
              onClick={triggerRefresh}
              disabled={data.loading}
              className="flex items-center gap-2 rounded-md border border-[var(--ct-border)] px-3 py-1.5 text-[var(--ct-fg)] transition-colors hover:bg-[var(--ct-hover)] disabled:opacity-60"
            >
              <BusySpinner icon={RefreshCw} size={13} spinning={data.loading} />
              Retry
            </button>
          </section>
        )}
        {activeTab === "inventory" && !meshOff && !graphReady && !unavailable && (
          <section aria-label="Mesh inventory" aria-busy="true" className="absolute inset-0 flex items-center justify-center">
            <Loader label="Loading mesh" />
          </section>
        )}
        {activeTab === "inventory" && !meshOff && graphReady && (
          <div ref={mapRef} className="absolute inset-0">
            <AccountMeshCanvas
              registryRefreshKey={registryRefreshKey}
              onAccountChanged={triggerRefresh}
              onRefresh={refreshAll}
              webCard={webCard}
              webStatus={deriveWebUiView(serve?.snapshot, serveLoading).label}
              thisMachine={data.thisMachine}
              peers={data.peers}
              services={data.services}
              loading={data.loading}
              error={data.error}
              recentPushes={recentPushes}
              onPeerClick={handlePeerClick}
              width={mapSize.width}
              height={mapSize.height}
              meshEnabled={config.mesh.enabled}
              onGoOnline={goOnline}
              tailscale={tailscale}
              onOpenPeerWebUi={openPeerWebUi}
              webUiBaseUrl={displayedWebUrl(webUiTarget.link)}
              webUiToken={config.api.token}
            />
          </div>
        )}
        {activeTab === "activity" && <ActivityTab active={activeTab === "activity"} />}
        {activeTab === "network" && (
          <NetworkTab triggerRefresh={triggerRefresh} />
        )}
        <ServiceDrawer
          state={drawerState}
          loading={data.loading}
          error={data.error}
          services={data.services}
          peers={data.peers}
          thisMachineName={data.thisMachine.name}
          thisMachineIp={data.thisMachine.ip}
          onClose={closeDrawer}
          onActionDone={() => setRefreshKey((k) => k + 1)}
          onAnnouncement={setMeshAnnouncement}
        />
        <div className="sr-only" role="status" aria-live="polite" aria-atomic="true" data-testid="mesh-panel-announcement">{meshAnnouncement}</div>

      </div>
    </div>
  );
}
