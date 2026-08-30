import { useEffect, useMemo, useRef, useState, useCallback } from "react";
import QRCode from "qrcode";
import { Check, Copy, ExternalLink, Info, Loader2, RefreshCw, ShieldCheck, Sparkles, X } from "lucide-react";
import { invoke } from "../../lib/bridge";
import { BusySpinner } from "../shared/BusyState";
import { IS_WEB, webJson } from "../../lib/webApi";
import logoUrl from "../../assets/logo.png";
import type { ApiConfig, AppConfig } from "../../lib/types";
import type { MeshTab, Service } from "./types";
import { MeshCanvas, type RecentPush } from "./MeshCanvas";
import { MeshInventoryList } from "./MeshInventoryList";
import { MeshInventoryModeToggle, type MeshInventoryMode } from "./MeshInventoryModeToggle";
import { ServiceDrawer, type DrawerState } from "./ServiceDrawer";
import { useMeshData } from "./useMeshData";
import { NetworkTab } from "./NetworkTab";
import { ActivityTab } from "./ActivityTab";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { MeshWebUiControl, type WebUiAction } from "../Titlebar/MeshWebUiControl";
import { UI_BUTTON, UI_INPUT, UI_PANEL, UI_TEXT } from "../shared/design";
import { LiquidTabRail } from "../shared/LiquidTabRail";
import {
  buildWebAccessUrls,
  planServeSetup,
  serveSetupTask,
  webLinkWithToken,
  webLinkWithTokenAndRelay,
  type ServeSetupPlan,
  type ServeStatus,
} from "../../lib/webAccess";

const TABS: MeshTab[] = ["inventory", "activity", "network"];

interface NetworkInfo {
  local_ip?: string | null;
  tailscale_ip?: string | null;
  tailscale_https_url?: string | null;
  api_port?: number;
}

interface WebUiTarget {
  link: string;
  usingFallback: boolean;
  port: number;
}

function buildWebUiTarget(config: AppConfig, network: NetworkInfo | null): WebUiTarget {
  const picked = buildWebAccessUrls(network, config.api.port);
  return {
    link: webLinkWithToken(picked.primary, config.api.token),
    usingFallback: picked.usingFallback,
    port: network?.api_port ?? config.api.port,
  };
}

interface ServeResult {
  ok: boolean;
  code: string;
  https_url?: string | null;
  warning?: string | null;
}

function WebUiModal({
  config,
  onClose,
  onExitMesh,
}: {
  config: AppConfig;
  onClose: () => void;
  onExitMesh: () => void;
}) {
  const [target, setTarget] = useState<WebUiTarget>(() => buildWebUiTarget(config, null));
  const link = target.link;
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null);
  const [copied, setCopied] = useState<"link" | "token" | null>(null);
  const [serve, setServe] = useState<ServeStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [setupMsg, setSetupMsg] = useState<string | null>(null);
  const [showInfo, setShowInfo] = useState(false);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      const [network, status] = await Promise.all([
        invoke<NetworkInfo>("get_network_info").catch(() => null),
        invoke<ServeStatus>("get_serve_status").catch(() => null),
      ]);
      if (cancelled) return;
      setTarget(buildWebUiTarget(config, network));
      setServe(status);
    }
    void load();
    return () => {
      cancelled = true;
    };
  }, [config]);

  const plan: ServeSetupPlan | null = serve
    ? planServeSetup(serve, target.port, serve.platform ?? "")
    : null;

  // Tailscale installed → flip on Serve natively in one tap; QR re-resolves to the https endpoint.
  const runEnable = useCallback(async () => {
    setBusy(true);
    setSetupMsg(null);
    try {
      const res = await invoke<ServeResult>("enable_web_access", {});
      if (res.ok) {
        const [network, status] = await Promise.all([
          invoke<NetworkInfo>("get_network_info").catch(() => null),
          invoke<ServeStatus>("get_serve_status").catch(() => null),
        ]);
        setTarget(buildWebUiTarget(config, network));
        setServe(status);
        setSetupMsg(res.warning ?? "HTTPS enabled via Tailscale.");
      } else {
        setSetupMsg(`Couldn't enable automatically (${res.code}). Try "Set up with AI".`);
      }
    } catch (e) {
      setSetupMsg(`Enable failed: ${String(e)}. Try "Set up with AI".`);
    } finally {
      setBusy(false);
    }
  }, [config]);

  // Not installed (or native enable failed) → spawn a Claude tab pre-loaded with the setup task and jump into it.
  const spawnSetupChat = useCallback(async () => {
    const body = {
      agent: "claude",
      title: "Set up Web Access",
      task: serveSetupTask(serve, target.port),
      activate: true,
    };
    try {
      if (IS_WEB) {
        await webJson("POST", "/api/agents/spawn", body);
      } else {
        const host = config.api.host === "0.0.0.0" ? "127.0.0.1" : config.api.host;
        await fetch(`http://${host}:${config.api.port}/api/agents/spawn`, {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            ...(config.api.token ? { Authorization: `Bearer ${config.api.token}` } : {}),
          },
          body: JSON.stringify(body),
        });
      }
      onExitMesh();
    } catch (e) {
      setSetupMsg(`Couldn't open setup chat: ${String(e)}`);
    }
  }, [serve, target.port, config, onExitMesh]);

  useEffect(() => {
    let cancelled = false;
    QRCode.toDataURL(link, {
      // High error correction tolerates the centered logo overlay without breaking scans.
      errorCorrectionLevel: "H",
      margin: 1,
      width: 320,
      color: {
        dark: "#0d1117",
        light: "#ffffff",
      },
    })
      .then((url) => {
        if (!cancelled) setQrDataUrl(url);
      })
      .catch(() => {
        if (!cancelled) setQrDataUrl(null);
      });
    return () => {
      cancelled = true;
    };
  }, [link]);

  const copyText = useCallback(async (text: string, kind: "link" | "token") => {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      const el = document.createElement("textarea");
      el.value = text;
      el.style.position = "fixed";
      el.style.left = "-9999px";
      document.body.appendChild(el);
      el.focus();
      el.select();
      document.execCommand("copy");
      document.body.removeChild(el);
    }
    setCopied(kind);
    window.setTimeout(() => setCopied(null), 1200);
  }, []);

  const hostLabel = link.replace(/^https?:\/\//, "").replace(/[#?]token=.*/, "");

  return (
    <div className="absolute inset-0 z-40 flex items-center justify-center bg-black/55 px-4 backdrop-blur-[2px] pt-[var(--ct-safe-top,0px)] pb-[var(--ct-safe-bottom,0px)]">
      <div
        data-testid="web-ui-modal"
        role="dialog"
        aria-modal="true"
        aria-label="Web UI access"
        className="w-full max-w-[360px] overflow-hidden rounded-2xl border border-[var(--ct-border)] bg-[var(--ct-bg)] shadow-2xl"
      >
        <div className="flex items-center justify-between border-b border-[var(--ct-border)] px-4 py-3">
          <div className="min-w-0">
            <div className="text-sm font-semibold text-[var(--ct-fg)]">Web UI</div>
            <div className={`${UI_TEXT.metaMuted} truncate`}>{hostLabel}</div>
          </div>
          <button
            type="button"
            onClick={onClose}
            className={`${UI_BUTTON.ghost} ${UI_BUTTON.icon} shrink-0`}
            title="Close"
          >
            <X size={15} />
          </button>
        </div>
        <div className="flex flex-col items-center gap-4 px-4 py-5">
          {/* CodeTerm-branded QR: gradient frame + centered logo (scan stays robust via EC level H). */}
          <div
            className="rounded-2xl p-[3px] shadow-[0_10px_30px_rgba(31,111,235,0.25)]"
            style={{ background: "linear-gradient(135deg, var(--ct-accent, #58a6ff) 0%, #7c5cff 100%)" }}
          >
            <div className="relative rounded-[15px] bg-white p-3">
              {qrDataUrl ? (
                <img src={qrDataUrl} alt="Web UI QR" className="block h-[228px] w-[228px]" />
              ) : (
                <div className="flex h-[228px] w-[228px] items-center justify-center text-xs text-slate-400">
                  Generating…
                </div>
              )}
              {qrDataUrl && (
                <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
                  <div className="rounded-[11px] bg-white p-1.5 shadow-[0_2px_8px_rgba(0,0,0,0.18)]">
                    <img src={logoUrl} alt="" className="h-9 w-9 rounded-lg" />
                  </div>
                </div>
              )}
            </div>
          </div>

          {/* Single-line link field — overflow scrolls right; tap selects all. Copy sits beside it. */}
          <div className="flex w-full items-stretch gap-2">
            <input
              type="text"
              readOnly
              value={link}
              data-testid="web-ui-link"
              onFocus={(e) => e.currentTarget.select()}
              className={`${UI_INPUT.base} ${UI_INPUT.compact} ct-text-mono min-w-0 flex-1`}
              aria-label="Web UI link"
            />
            <button
              type="button"
              data-testid="web-ui-copy"
              onClick={() => void copyText(link, "link")}
              className={`${UI_BUTTON.ghost} ${UI_BUTTON.icon} shrink-0`}
              title={copied === "link" ? "Copied" : "Copy link"}
            >
              {copied === "link" ? <Check size={15} className="text-[var(--ct-green,#3fb950)]" /> : <Copy size={15} />}
            </button>
          </div>

          {!target.usingFallback && (
            <div
              data-testid="web-ui-https-active"
              className="flex w-full items-center gap-1.5 text-[11px] text-[var(--ct-green,#3fb950)]"
            >
              <ShieldCheck size={13} className="shrink-0" />
              <span className="truncate">Secure HTTPS link via Tailscale Serve.</span>
            </div>
          )}

          {target.usingFallback && (
            <div className="w-full space-y-2" data-testid="web-ui-setup">
              <div className="flex items-start gap-1.5 text-[11px] text-[var(--ct-yellow,#d29922)] leading-snug">
                <Info size={13} className="mt-px shrink-0" />
                <span>
                  This is a raw <span className="font-mono">http://</span> address — phones can't
                  use the microphone on it and it isn't encrypted. Set up Tailscale for a clean
                  HTTPS link.
                </span>
              </div>

              {plan?.state === "can-enable" && (
                <button
                  type="button"
                  data-testid="web-ui-enable-https"
                  onClick={() => void runEnable()}
                  disabled={busy}
                  className={`${UI_BUTTON.accent} ${UI_BUTTON.lg} w-full gap-1.5 disabled:opacity-50`}
                >
                  {busy ? <BusySpinner icon={Loader2} size={14} /> : <ShieldCheck size={14} />}
                  {busy ? "Enabling HTTPS…" : "Enable HTTPS (Tailscale Serve)"}
                </button>
              )}

              {plan?.state !== "can-enable" && (
                <button
                  type="button"
                  data-testid="web-ui-setup-ai"
                  onClick={() => void spawnSetupChat()}
                  className={`${UI_BUTTON.accent} ${UI_BUTTON.lg} w-full gap-1.5`}
                >
                  <Sparkles size={14} />
                  Set up with AI
                </button>
              )}

              {plan?.state === "can-enable" && (
                <button
                  type="button"
                  data-testid="web-ui-setup-ai-alt"
                  onClick={() => void spawnSetupChat()}
                  className={`${UI_BUTTON.ghost} ${UI_BUTTON.sm} w-full gap-1.5`}
                >
                  <Sparkles size={13} />
                  Trouble? Set up with AI instead
                </button>
              )}

              {setupMsg && (
                <p data-testid="web-ui-setup-msg" className={`${UI_TEXT.metaMuted} leading-snug`}>
                  {setupMsg}
                </p>
              )}

              <button
                type="button"
                onClick={() => setShowInfo((v) => !v)}
                className={`${UI_TEXT.metaMuted} flex items-center gap-1 hover:text-[var(--ct-fg)]`}
              >
                <Info size={11} />
                What is Tailscale &amp; Serve?
              </button>
              {showInfo && (
                <p className={`${UI_TEXT.metaMuted} leading-snug`}>
                  Tailscale is a private mesh VPN that links your own devices. "Serve" publishes
                  this CodeTerm over HTTPS at a stable <span className="font-mono">*.ts.net</span>{" "}
                  address, so your phone gets a secure, mic-capable link with no port juggling. Free
                  for personal use — <span className="font-mono">tailscale.com</span>.
                </p>
              )}
            </div>
          )}

          <div className="grid w-full grid-cols-2 gap-2">
            <button
              type="button"
              data-testid="web-ui-open"
              onClick={() => void invoke("open_url", { url: link })}
              className={`${UI_BUTTON.accent} ${UI_BUTTON.lg} gap-1.5`}
            >
              <ExternalLink size={14} />
              Open
            </button>
            <button
              type="button"
              data-testid="web-ui-copy-token"
              onClick={() => void copyText(config.api.token ?? "", "token")}
              disabled={!config.api.token}
              className={`${UI_BUTTON.ghost} ${UI_BUTTON.lg} gap-1.5 disabled:opacity-40`}
            >
              {copied === "token" ? <Check size={14} className="text-[var(--ct-green,#3fb950)]" /> : <Copy size={14} />}
              {copied === "token" ? "Copied" : "Copy token"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
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
  const [inventoryMode, setInventoryMode] = useState<MeshInventoryMode>("list");
  const [refreshKey, setRefreshKey] = useState(0);
  const data = useMeshData(refreshKey);
  const [recentPushes, setRecentPushes] = useState<RecentPush[]>([]);
  const [webUiOpen, setWebUiOpen] = useState(false);
  const [serve, setServe] = useState<ServeStatus | null>(null);
  const [serveLoading, setServeLoading] = useState(true);

  const triggerRefresh = useCallback(() => setRefreshKey((k) => k + 1), []);

  useEffect(() => {
    let unlistenHealth: (() => void) | null = null;
    let unlistenPush: (() => void) | null = null;
    import("../../lib/bridge").then(({ listen }) => {
      listen("service-health-changed", triggerRefresh).then((unlisten) => { unlistenHealth = unlisten; });
      listen<{ source_id: string; target: string }>("service-pushed", (event) => {
        setRecentPushes((current) => [...current, { ...event.payload, at: Date.now() }].slice(-20));
      }).then((unlisten) => { unlistenPush = unlisten; });
    });
    return () => {
      unlistenHealth?.();
      unlistenPush?.();
    };
  }, [triggerRefresh]);

  const loadServe = useCallback(async () => {
    const status = await invoke<ServeStatus>("get_serve_status").catch(() => null);
    setServe(status);
    setServeLoading(false);
    return status;
  }, []);

  useEffect(() => {
    void loadServe();
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
  const [selectedResourceKey, setSelectedResourceKey] = useState<string | null>(null);
  const mapRef = useRef<HTMLDivElement>(null);
  const [mapSize, setMapSize] = useState({ width: 800, height: 600 });
  const inventoryPeerIssues = useMemo(
    () => data.peers
      .filter((peer) => !peer.pending && (peer.services_loaded === false || Boolean(peer.services_error)))
      .map((peer) => ({
        machineId: peer.instance_id,
        machineName: peer.name,
        freshness: peer.online ? "stale" as const : "offline" as const,
        partialError: peer.services_error,
      })),
    [data.peers],
  );
  const handleServiceClick = useCallback((s: Service, resourceKey?: string) => {
    setSelectedResourceKey(resourceKey ?? null);
    setDrawerState({
      kind: "service",
      id: s.id,
      sourcePeerId: s.source_peer_id ?? s.source_peer ?? null,
    });
  }, []);

  const handlePeerClick = useCallback((name: string) => {
    setDrawerState({ kind: "peer", id: name });
  }, []);

  useEffect(() => {
    const element = mapRef.current;
    if (!element || activeTab !== "inventory" || inventoryMode !== "graph") return;
    const observer = new ResizeObserver(() => {
      const bounds = element.getBoundingClientRect();
      if (bounds.width <= 0 || bounds.height <= 0) return;
      setMapSize((current) => Math.abs(current.width - bounds.width) < 1 && Math.abs(current.height - bounds.height) < 1
        ? current
        : { width: bounds.width, height: bounds.height });
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [activeTab, inventoryMode]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
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
        className={`${UI_PANEL.toolbar} ct-section-toolbar gap-0 px-4 py-0 bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] text-[var(--ct-fg)] whitespace-nowrap`}
        style={touchUi ? { height: "auto", minHeight: "3.25rem" } : undefined}
      >
        <div className="ct-section-toolbar__identity flex items-center gap-3 mr-4 shrink-0">
          <strong>Mesh</strong>
          <span className={`${UI_TEXT.meta} opacity-70`}>
            {data.error
              ? `error: ${data.error}`
              : `${data.peers.length} peer${data.peers.length === 1 ? "" : "s"} · ${data.services.length} service${data.services.length === 1 ? "" : "s"}`}
          </span>
          <button
            onClick={() => setRefreshKey((k) => k + 1)}
            className={`flex ${touchUi ? "h-9 w-9" : "h-7 w-7"} shrink-0 items-center justify-center p-0 text-[var(--ct-muted-fg)] transition-colors hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] active:scale-95`}
            style={{ borderRadius: "9999px" }}
            title="Refresh"
          >
            <BusySpinner icon={RefreshCw} size={touchUi ? 16 : 13} spinning={data.loading} />
          </button>
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
        {activeTab === "inventory" && (
          <>
            {inventoryMode === "list" ? (
              <MeshInventoryList
                resources={data.services}
                thisMachineName={data.thisMachine.name}
                peerIssues={inventoryPeerIssues}
                loading={data.loading}
                error={data.error}
                selectedResourceKey={selectedResourceKey}
                onSelectResource={handleServiceClick}
                onRefresh={triggerRefresh}
                headerActions={
                  <MeshWebUiControl
                    snapshot={serve?.snapshot ?? null}
                    loading={serveLoading}
                    onOpenSurface={() => setWebUiOpen(true)}
                    accessUrl={serve?.snapshot?.url
                      ? webLinkWithTokenAndRelay(serve.snapshot.url, config.api.token, null)
                      : null}
                    onOpenUrl={(url) => void invoke("open_url", { url })}
                    onAction={runWebUiAction}
                    touchUi={touchUi}
                    menuAlign="right"
                  />
                }
              />
            ) : (
              <div ref={mapRef} className="absolute inset-0">
                <div className="absolute left-3 top-3 z-20 rounded-lg border border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] px-2 py-1.5 shadow-sm">
                  <MeshWebUiControl
                    snapshot={serve?.snapshot ?? null}
                    loading={serveLoading}
                    onOpenSurface={() => setWebUiOpen(true)}
                    accessUrl={serve?.snapshot?.url
                      ? webLinkWithTokenAndRelay(serve.snapshot.url, config.api.token, null)
                      : null}
                    onOpenUrl={(url) => void invoke("open_url", { url })}
                    onAction={runWebUiAction}
                    touchUi={touchUi}
                  />
                </div>
                <MeshCanvas
                  thisMachine={data.thisMachine}
                  peers={data.peers}
                  services={data.services}
                  recentPushes={recentPushes}
                  onPeerClick={handlePeerClick}
                  width={mapSize.width}
                  height={mapSize.height}
                  onAddPeerClick={() => setActiveTab("network")}
                  meshEnabled={config.mesh.enabled}
                  onGoOnline={goOnline}
                  onOpenPeerWebUi={(url) => void invoke("open_url", { url })}
                  webUiBaseUrl={serve?.snapshot?.url ?? null}
                  webUiToken={config.api.token}
                />
              </div>
            )}
            <div className="absolute bottom-4 left-1/2 z-10 -translate-x-1/2">
              <MeshInventoryModeToggle mode={inventoryMode} onChange={setInventoryMode} />
            </div>
          </>
        )}
        {activeTab === "activity" && <ActivityTab active={activeTab === "activity"} />}
        {activeTab === "network" && (
          <NetworkTab data={data} triggerRefresh={triggerRefresh} />
        )}
        <ServiceDrawer
          state={drawerState}
          services={data.services}
          peers={data.peers}
          thisMachineName={data.thisMachine.name}
          thisMachineIp={data.thisMachine.ip}
          onClose={() => setDrawerState(null)}
          onOpenResource={(service, resourceKey) => handleServiceClick(service, resourceKey)}
          onActionDone={() => setRefreshKey((k) => k + 1)}
          onAnnouncement={setMeshAnnouncement}
        />
        <div className="sr-only" role="status" aria-live="polite" aria-atomic="true" data-testid="mesh-panel-announcement">{meshAnnouncement}</div>
        {webUiOpen && (
          <WebUiModal
            config={config}
            onClose={() => setWebUiOpen(false)}
            onExitMesh={() => {
              setWebUiOpen(false);
              onClose();
            }}
          />
        )}
      </div>
    </div>
  );
}
