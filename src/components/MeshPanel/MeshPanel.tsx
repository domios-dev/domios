import { useMeshConnections } from "./useMeshConnections";
import { nativeBridge } from "../../lib/nativeBridge";
import { useEffect, useLayoutEffect, useMemo, useRef, useState, useCallback } from "react";
import { RefreshCw } from "lucide-react";
import { invoke, listen } from "../../lib/bridge";
import { BusySpinner, BusyState } from "../shared/BusyState";
import type { ApiConfig, AppConfig } from "../../lib/types";
import type { MeshTab } from "./types";
import { MeshCanvas, type CanvasCloud, type RecentPush } from "./MeshCanvas";
import { useCloudMachines } from "./useCloudMachines";
import { cloudHoursPurchasedEvent } from "../../lib/cloudMachines/analytics";
import { cloudCreateDecision, type CloudFailurePresentation } from "../../lib/cloudMachines/stateModel";
import type { TopUpMode } from "../../lib/cloudMachines/topUpModel";
import type { CloudReceipt } from "../../lib/cloudMachines/types";
import { recordProductEvent } from "../../lib/productAnalytics";
import { CloudTopUpDialog, type PendingCloudTopUpCheckout } from "../billing/CloudTopUpDialog";
import { CloudTopUpReturnDialog } from "../billing/CloudTopUpReturnDialog";
import { useAccountPlan } from "../Settings/useAccountPlan";
import { setSidebarMachineFilter } from "../Sidebar/sidebarMachineFilter";
import { accountCanvasMachineCounts, accountCanvasPeers } from "./accountCanvas";
import { countProjectedMeshMachines, deduplicateMeshInputs } from "./meshMachineListModel";
import { useCanvasAccount } from "./useCanvasAccount";
import { useMeshAccount } from "./useMeshAccount";
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

type ServeResult = import("../../../gen/ServeResult").ServeResult;

interface Props {
  api: ApiConfig;
  config: AppConfig;
  onConfigUpdate: (config: AppConfig) => Promise<void>;
  onClose: () => void;
  tabRequest?: { tab: MeshTab; newMachine?: boolean; seq?: number };
}

export function MeshPanel({ config, onClose, tabRequest }: Props) {
  const touchUi = useTouchOptimizedUi();
  const account = useMeshAccount();
  const accountPlan = useAccountPlan(account.presence === "signed_in" ? (account.uid ?? null) : null);
  const connecting = useMeshConnections(config.mesh.enabled);
  const goOnline = useCallback(async () => {
    await invoke("mesh_set_enabled", { enabled: true });
  }, []);
  const [activeTab, setActiveTab] = useState<MeshTab>(tabRequest?.tab ?? "inventory");
  const [slotOpen, setSlotOpen] = useState(Boolean(tabRequest?.newMachine));
  useLayoutEffect(() => {
    if (!tabRequest) return;
    setActiveTab(tabRequest.tab);
    if (tabRequest.newMachine) setSlotOpen(true);
  }, [tabRequest]);
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
  const inventoryShown = activeTab === "inventory" && !meshOff && graphReady;
  const canvasAccount = useCanvasAccount(data.peers, triggerRefresh, registryRefreshKey, account, inventoryShown);
  const canvasPeers = useMemo(() => accountCanvasPeers(canvasAccount.canvas, data.peers), [canvasAccount.canvas, data.peers]);

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
  const [topUpDialog, setTopUpDialog] = useState<{ mode: TopUpMode; machine?: CloudReceipt; size?: string } | null>(null);
  const [pendingCheckout, setPendingCheckout] = useState<PendingCloudTopUpCheckout | null>(null);
  const [returnDialog, setReturnDialog] = useState<{
    addedHours: number;
    newBalance: number;
    rolloverSentence: string | null;
    wakeTarget?: { machineId: string; peerId: string; machineName: string };
    creating?: boolean;
  } | null>(null);
  const cloud = useCloudMachines(
    account.presence === "signed_in" && (inventoryShown || drawerState?.kind === "cloud"),
    refreshAll,
  );
  const machineCounts = useMemo(() => {
    const base = accountCanvasMachineCounts(canvasAccount.canvas, canvasPeers);
    if (cloud.machines.length === 0) return base;
    const { activeCloud, nonCloudPeers, accountNodes } = deduplicateMeshInputs(
      canvasPeers,
      canvasAccount.canvas,
      cloud.machines,
    );
    const connectedCloud = activeCloud.filter((machine) => machine.state === "awake" || machine.state === "draining").length;
    const connectedPeers = nonCloudPeers.filter(
      (peer) => !peer.pending && peer.online && canvasAccount.canvas.peerFooters.get(peer.instance_id)?.row.status !== "signed_out",
    ).length;
    const connectedNodes = accountNodes.filter((node) => node.online === true).length;
    return {
      total: countProjectedMeshMachines(canvasPeers, canvasAccount.canvas, cloud.machines),
      connected: 1 + connectedPeers + connectedCloud + connectedNodes,
    };
  }, [canvasAccount.canvas, canvasPeers, cloud.machines]);
  const isFreePlan = accountPlan.plan !== null && !accountPlan.plan.fullAccess && accountPlan.plan.plan === "free";

  useEffect(() => {
    if (!pendingCheckout || !cloud.usage) return;
    const delta = Math.round((cloud.usage.remaining - pendingCheckout.priorRemaining) * 10) / 10;
    if (delta <= 0) return;
    const checkout = pendingCheckout;
    setPendingCheckout(null);
    void recordProductEvent(cloudHoursPurchasedEvent(checkout.packId, checkout.amountCents));
    if (checkout.mode === "wake" && checkout.machineId) {
      const targetMachine = cloud.machines.find((item) => item.machine_id === checkout.machineId);
      const peerId = targetMachine?.peer_instance_id ?? targetMachine?.installation_id ?? checkout.machineId;
      void cloud.wake(checkout.machineId);
      setReturnDialog({
        addedHours: delta || checkout.expectedHours || 0,
        newBalance: cloud.usage.remaining,
        rolloverSentence: checkout.rolloverSentence,
        wakeTarget: { machineId: checkout.machineId, peerId, machineName: checkout.machineName ?? targetMachine?.name ?? "Machine" },
      });
    } else if (checkout.mode === "free" && checkout.size) {
      setSlotOpen(false);
      void cloud.create(checkout.size);
      setReturnDialog({
        addedHours: delta || checkout.expectedHours || 0,
        newBalance: cloud.usage.remaining,
        rolloverSentence: checkout.rolloverSentence,
        creating: true,
      });
    } else {
      setReturnDialog({
        addedHours: delta || checkout.expectedHours || 0,
        newBalance: cloud.usage.remaining,
        rolloverSentence: checkout.rolloverSentence,
      });
    }
  }, [pendingCheckout, cloud]);

  const handleCreateCloud = useCallback((size: string) => {
    const decision = cloudCreateDecision({
      planId: accountPlan.plan?.plan ?? null,
      remaining: cloud.usage ? cloud.usage.remaining : null,
      fullAccess: accountPlan.plan?.fullAccess === true,
    });
    if (decision === "free_paywall") {
      setTopUpDialog({ mode: "free", size });
      return;
    }
    if (decision === "top_up") {
      setTopUpDialog({ mode: "topup", size });
      return;
    }
    setSlotOpen(false);
    void cloud.create(size);
  }, [accountPlan.plan, cloud]);

  const handleCloudFailureAction = useCallback((action: CloudFailurePresentation["action"]) => {
    if (action === "retry") {
      const size = cloud.pendingCreate?.size ?? "small";
      void cloud.create(size);
      return;
    }
    if (action === "top_up") {
      setTopUpDialog({ mode: "topup", size: cloud.pendingCreate?.size });
      return;
    }
    window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "settings:plans" } }));
  }, [cloud]);

  const canvasCloud = useMemo<CanvasCloud | undefined>(() => {
    if (account.presence !== "signed_in") return undefined;
    return {
      machines: cloud.machines,
      usage: cloud.usage,
      isFree: isFreePlan,
      slotOpen,
      pendingCreate: cloud.pendingCreate,
      selectedMachineId: drawerState?.kind === "cloud" ? drawerState.machineId : null,
      busyMachineId: cloud.busyMachineId,
      onOpenSlot: () => {
        cloud.clearPendingCreate();
        setSlotOpen(true);
      },
      onCloseSlot: () => setSlotOpen(false),
      onCreateCloud: handleCreateCloud,
      onPairComputer: () => {
        setSlotOpen(false);
        setActiveTab("network");
      },
      onSelectMachine: (machineId) => setDrawerState({ kind: "cloud", machineId }),
      onTopUpMachine: (machine) => setTopUpDialog({ mode: "wake", machine }),
      onSleepMachine: cloud.sleep,
      onWakeMachine: cloud.wake,
      onFailureAction: handleCloudFailureAction,
    };
  }, [account.presence, cloud, isFreePlan, slotOpen, drawerState, handleCreateCloud, handleCloudFailureAction]);

  const cloudDrawerControls = useMemo(() => ({
    machines: cloud.machines,
    usage: cloud.usage,
    planId: accountPlan.plan?.plan ?? null,
    idleSleepMinutes: config.mesh.cloud_idle_sleep_minutes || 15,
    busyMachineId: cloud.busyMachineId,
    actionError: cloud.actionError,
    onTopUp: (machine: CloudReceipt, forWake: boolean) => setTopUpDialog({ mode: forWake ? "wake" : "topup", machine }),
    onRedeem: cloud.redeem,
    onSleep: cloud.sleep,
    onWake: cloud.wake,
    onDelete: async (machineId: string) => {
      const res = await cloud.remove(machineId);
      return res.ok;
    },
  }), [cloud, accountPlan.plan?.plan, config.mesh.cloud_idle_sleep_minutes]);
  const mapRef = useRef<HTMLDivElement>(null);
  const [mapSize, setMapSize] = useState({ width: 800, height: 600 });
  const handlePeerClick = useCallback((id: string) => {
    setDrawerState({ kind: "peer", id });
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
          <div className={UI_TEXT.meta}>
            {header.kind === "off"
              ? <span role="status">Mesh is off.</span>
              : header.kind === "unavailable"
              ? <span role="status">Mesh data is unavailable.</span>
              : header.kind === "loading"
                ? <DeferredLoader state="present" aria-busy="true"><BusyState inline label="Loading peers" phase="work" /></DeferredLoader>
                : inventoryShown && account.presence === "signed_in" && canvasAccount.canvas.card?.kind !== "loading"
                  ? <>{machineCounts.total} machine{machineCounts.total === 1 ? "" : "s"} <span className="text-[var(--ct-muted)]">· {machineCounts.connected} connected</span></>
                  : null}
            {connecting > 0 && <span role="status" className="ml-2"><BusyState inline label={`${connecting} peer${connecting === 1 ? "" : "s"} connecting`} phase="work" /></span>}
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
        {inventoryShown && (
          <div ref={mapRef} className="absolute inset-0">
            <MeshCanvas
              account={canvasAccount}
              cloud={canvasCloud}
              touch={touchUi}
              onRefresh={() => {
                refreshAll();
                void cloud.refreshAll();
              }}
              webCard={webCard}
              webStatus={deriveWebUiView(serve?.snapshot, serveLoading).label}
              thisMachine={data.thisMachine}
              peers={canvasPeers}
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
              selectedPeerId={drawerState?.kind === "peer" ? drawerState.id : null}
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
          cloudDrawer={cloudDrawerControls}
        />
        {topUpDialog && (
          <CloudTopUpDialog
            mode={topUpDialog.mode}
            usage={cloud.usage}
            currentPlan={accountPlan.plan?.plan ?? null}
            machineId={topUpDialog.machine?.machine_id}
            machineName={topUpDialog.machine?.name}
            size={topUpDialog.size}
            onClose={() => setTopUpDialog(null)}
            onCheckoutStarted={(pending) => setPendingCheckout(pending)}
          />
        )}
        {returnDialog && (
          <CloudTopUpReturnDialog
            addedHours={returnDialog.addedHours}
            newBalance={returnDialog.newBalance}
            rolloverSentence={returnDialog.rolloverSentence}
            creatingMachine={returnDialog.creating}
            wakeTarget={
              returnDialog.wakeTarget
                ? {
                    machineName: returnDialog.wakeTarget.machineName,
                    onOpen: () => {
                      const target = returnDialog.wakeTarget!;
                      setReturnDialog(null);
                      setSidebarMachineFilter({ id: target.peerId, name: target.machineName });
                      onClose();
                    },
                  }
                : null
            }
            onClose={() => setReturnDialog(null)}
          />
        )}
        <div className="sr-only" role="status" aria-live="polite" aria-atomic="true" data-testid="mesh-panel-announcement">{meshAnnouncement}</div>

      </div>
    </div>
  );
}
