import { useEffect, useRef, useState, useCallback } from "react";
import QRCode from "qrcode";
import { ChevronUp, ChevronDown, Copy, ExternalLink, X } from "lucide-react";
import { invoke } from "../../lib/bridge";
import type { ApiConfig, AppConfig } from "../../lib/types";
import type { MeshTab, Service } from "./types";
import { MeshCanvas, type RecentPush } from "./MeshCanvas";
import { ServicesTable } from "./ServicesTable";
import { CronsTable } from "./CronsTable";
import { ServiceDrawer, type DrawerState } from "./ServiceDrawer";
import { useMeshData } from "./useMeshData";
import { NetworkTab } from "./NetworkTab";
import { ActivityTab } from "./ActivityTab";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { tabClass, UI_BUTTON, UI_INPUT, UI_PANEL, UI_TEXT } from "../shared/design";

const TABS: MeshTab[] = ["map", "activity", "network"];

const TABLES_COLLAPSED_HEIGHT = 36;
const TABLES_EXPANDED_HEIGHT = 240;

interface DrawerProps {
  cronsCount: number;
  servicesCount: number;
  open: boolean;
  onToggle: () => void;
  services: Service[];
  thisMachineName: string;
  onServiceClick: (s: Service) => void;
}

const DRAWER_BTN =
  `${UI_BUTTON.ghost} ${UI_BUTTON.compact} gap-1 bg-transparent border-none cursor-pointer`;

function CollapsibleTablesDrawer({
  cronsCount,
  servicesCount,
  open,
  onToggle,
  services,
  thisMachineName,
  onServiceClick,
}: DrawerProps) {
  const Chevron = open ? ChevronDown : ChevronUp;
  return (
    <div
      className="absolute inset-x-0 bottom-0 z-10 flex flex-col bg-[var(--ct-bg)]/95 backdrop-blur-sm border-t border-[var(--ct-border)] shadow-[0_-4px_8px_rgba(0,0,0,0.15)]"
      style={{
        height: open ? TABLES_EXPANDED_HEIGHT + TABLES_COLLAPSED_HEIGHT : TABLES_COLLAPSED_HEIGHT,
        maxHeight: "60%",
      }}
    >
      <div
        className="flex items-center px-3 bg-[var(--ct-titlebar-bg)]/80 border-b border-[var(--ct-border)] flex-shrink-0"
        style={{ height: TABLES_COLLAPSED_HEIGHT }}
      >
        <button className={DRAWER_BTN} onClick={onToggle}>
          <Chevron size={12} />
          Crons ({cronsCount})
        </button>
        <button data-testid="mesh-services-toggle" className={DRAWER_BTN} onClick={onToggle}>
          <Chevron size={12} />
          Services ({servicesCount})
        </button>
      </div>
      {open && (
        <div className="flex-1 min-h-0 overflow-auto flex">
          <CronsTable
            services={services}
            thisMachineName={thisMachineName}
            onServiceClick={onServiceClick}
          />
          <div className="w-px bg-[var(--ct-accent)]/30 shadow-[0_0_4px_var(--ct-accent)]" />
          <ServicesTable
            services={services}
            thisMachineName={thisMachineName}
            onServiceClick={onServiceClick}
          />
        </div>
      )}
    </div>
  );
}

interface NetworkInfo {
  local_ip?: string | null;
  tailscale_ip?: string | null;
  tailscale_https_url?: string | null;
  api_port?: number;
}

function buildWebUiLink(config: AppConfig, network: NetworkInfo | null): string {
  const port = network?.api_port ?? config.api.port;
  const token = config.api.token ? `#token=${encodeURIComponent(config.api.token)}` : "";
  if (network?.tailscale_https_url) {
    return `${network.tailscale_https_url}/web/${token}`;
  }
  const host = network?.tailscale_ip || network?.local_ip || "127.0.0.1";
  return `http://${host}:${port}/web/${token}`;
}

function WebUiModal({
  config,
  onClose,
}: {
  config: AppConfig;
  onClose: () => void;
}) {
  const [link, setLink] = useState(() => buildWebUiLink(config, null));
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null);
  const [copied, setCopied] = useState<"link" | "token" | null>(null);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      const network = await invoke<NetworkInfo>("get_network_info").catch(() => null);
      if (cancelled) return;
      setLink(buildWebUiLink(config, network));
    }
    void load();
    return () => {
      cancelled = true;
    };
  }, [config]);

  useEffect(() => {
    let cancelled = false;
    QRCode.toDataURL(link, {
      margin: 1,
      width: 260,
      color: {
        dark: "#10131f",
        light: "#f8fafc",
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

  return (
    <div className="absolute inset-0 z-40 flex items-center justify-center bg-black/45 px-4">
      <div
        data-testid="web-ui-modal"
        className="w-full max-w-[360px] rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)] shadow-2xl"
      >
        <div className="flex items-center justify-between border-b border-[var(--ct-border)] px-4 py-2.5">
          <div>
            <div className="text-sm font-medium text-[var(--ct-fg)]">Web UI</div>
            <div className={UI_TEXT.metaMuted}>
              {link.replace(/^https?:\/\//, "").replace(/[#?]token=.*/, "")}
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            className={`${UI_BUTTON.ghost} ${UI_BUTTON.icon}`}
            title="Close"
          >
            <X size={15} />
          </button>
        </div>
        <div className="flex flex-col items-center gap-3 px-4 py-4">
          <div className="flex h-[276px] w-[276px] items-center justify-center rounded-md bg-white p-2">
            {qrDataUrl ? (
              <img src={qrDataUrl} alt="Web UI QR" className="h-[260px] w-[260px]" />
            ) : (
              <div className="text-xs text-slate-500">QR</div>
            )}
          </div>
          <div className={`${UI_INPUT.base} ${UI_INPUT.compact} ct-text-mono`}>
            {link}
          </div>
          <div className="grid w-full grid-cols-3 gap-2">
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
              data-testid="web-ui-copy"
              onClick={() => void copyText(link, "link")}
              className={`${UI_BUTTON.ghost} ${UI_BUTTON.lg} gap-1.5`}
            >
              <Copy size={14} />
              {copied === "link" ? "Copied" : "Copy link"}
            </button>
            <button
              type="button"
              data-testid="web-ui-copy-token"
              onClick={() => void copyText(config.api.token ?? "", "token")}
              disabled={!config.api.token}
              className={`${UI_BUTTON.ghost} ${UI_BUTTON.lg} gap-1.5 disabled:opacity-40`}
            >
              <Copy size={14} />
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
  if (t === "map" || t === "activity" || t === "network") return t;
  return "map";
}

export function MeshPanel({ config, onConfigUpdate, onClose }: Props) {
  const touchUi = useTouchOptimizedUi();
  const goOnline = async () => {
    await onConfigUpdate({ ...config, mesh: { ...config.mesh, enabled: true } });
  };
  const [activeTab, setActiveTab] = useState<MeshTab>(() => normalizeTab("map"));
  const [refreshKey, setRefreshKey] = useState(0);
  const data = useMeshData(refreshKey);
  const [recentPushes, setRecentPushes] = useState<RecentPush[]>([]);
  const [webUiOpen, setWebUiOpen] = useState(false);

  const triggerRefresh = useCallback(() => setRefreshKey((k) => k + 1), []);

  useEffect(() => {
    let unlistenHealth: (() => void) | null = null;
    let unlistenPush: (() => void) | null = null;
    import("../../lib/bridge").then(({ listen }) => {
      listen<{ service_id: string; status: string }>("service-health-changed", () => {
        triggerRefresh();
      }).then((fn) => { unlistenHealth = fn; });
      listen<{ source_id: string; target: string }>("service-pushed", (ev) => {
        setRecentPushes((prev) =>
          [...prev, { ...ev.payload, at: Date.now() }].slice(-20)
        );
      }).then((fn) => { unlistenPush = fn; });
    });
    return () => { unlistenHealth?.(); unlistenPush?.(); };
  }, [triggerRefresh]);

  const mapRef = useRef<HTMLDivElement>(null);
  const [mapWidth, setMapWidth] = useState(800);
  const [mapHeight, setMapHeight] = useState(600);
  const [drawerState, setDrawerState] = useState<DrawerState>(null);
  const [tablesOpen, setTablesOpen] = useState(false);

  useEffect(() => {
    const el = mapRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const r = el.getBoundingClientRect();
      if (r.width > 0) setMapWidth(r.width);
      if (r.height > 0) setMapHeight(r.height);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [activeTab]);

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
        setActiveTab("map");
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

  const handlePeerClick = (name: string) =>
    setDrawerState({ kind: "peer", id: name });
  const handleServiceClick = (s: Service) =>
    setDrawerState({ kind: "service", id: s.id });
  return (
    <div className="absolute inset-0 flex flex-col bg-[var(--ct-bg)] pt-[var(--ct-safe-top,0px)]">
      <div className={`${UI_PANEL.toolbar} gap-0 px-4 py-0 bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] text-[var(--ct-fg)] ct-hscroll whitespace-nowrap`}>
        <div className="flex items-center gap-3 mr-4 shrink-0">
          <strong>Mesh</strong>
          <span className={`${UI_TEXT.meta} opacity-70`}>
            {data.error
              ? `error: ${data.error}`
              : `${data.peers.length} peer${data.peers.length === 1 ? "" : "s"} · ${data.services.length} service${data.services.length === 1 ? "" : "s"}`}
          </span>
          <button
            onClick={() => setRefreshKey((k) => k + 1)}
            className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact} border-none ${data.loading ? "animate-spin" : ""}`}
            title="Refresh"
          >
            ↻
          </button>
        </div>
        <div className="flex items-center gap-0 border-l border-[var(--ct-border)] pl-2">
          {TABS.map((tab) => (
            <button
              key={tab}
              data-testid={`mesh-tab-${tab}`}
              onClick={() => setActiveTab(tab)}
              className={tabClass(activeTab === tab, touchUi)}
            >
              {tab}
            </button>
          ))}
        </div>
      </div>
      <div className="relative flex-1 min-h-0 overflow-hidden">
        {activeTab === "map" && (
          <>
            <div ref={mapRef} className="absolute inset-0">
              <MeshCanvas
                thisMachine={data.thisMachine}
                peers={data.peers}
                services={data.services}
                recentPushes={recentPushes}
                onPeerClick={handlePeerClick}
                width={mapWidth}
                height={mapHeight}
                onAddPeerClick={() => setActiveTab("network")}
                meshEnabled={config.mesh.enabled}
                onGoOnline={goOnline}
                onOpenWebUi={() => setWebUiOpen(true)}
              />
            </div>
            <CollapsibleTablesDrawer
              cronsCount={data.services.filter((s) => s.kind === "cron").length}
              servicesCount={data.services.filter((s) => s.kind === "daemon").length}
              open={tablesOpen}
              onToggle={() => setTablesOpen((v) => !v)}
              services={data.services}
              thisMachineName={data.thisMachine.name}
              onServiceClick={handleServiceClick}
            />
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
          onActionDone={() => setRefreshKey((k) => k + 1)}
        />
        {webUiOpen && <WebUiModal config={config} onClose={() => setWebUiOpen(false)} />}
      </div>
    </div>
  );
}
