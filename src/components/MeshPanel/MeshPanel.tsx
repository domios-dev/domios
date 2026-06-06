import { useEffect, useRef, useState, useCallback } from "react";
import { ChevronUp, ChevronDown } from "lucide-react";
import type { ApiConfig, AppConfig } from "../../lib/types";
import type { MeshTab, Service } from "./types";
import { MeshCanvas, type RecentPush } from "./MeshCanvas";
import { ServicesTable } from "./ServicesTable";
import { CronsTable } from "./CronsTable";
import { ServiceDrawer, type DrawerState } from "./ServiceDrawer";
import { useMeshData } from "./useMeshData";
import { NetworkTab } from "./NetworkTab";
import { ActivityTab } from "./ActivityTab";

const TAB_BASE =
  "px-3 py-2 text-xs border-b-2 bg-transparent cursor-pointer capitalize";
const TAB_ACTIVE = `${TAB_BASE} border-[var(--ct-accent)] text-[var(--ct-fg)]`;
const TAB_INACTIVE = `${TAB_BASE} border-transparent text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]`;

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
  "flex items-center gap-1 px-2 py-1 bg-transparent border-none cursor-pointer text-[11px] uppercase tracking-wider text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]";

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
  const goOnline = async () => {
    await onConfigUpdate({ ...config, mesh: { ...config.mesh, enabled: true } });
  };
  const [activeTab, setActiveTab] = useState<MeshTab>(() => normalizeTab("map"));
  const [refreshKey, setRefreshKey] = useState(0);
  const data = useMeshData(refreshKey);
  const [recentPushes, setRecentPushes] = useState<RecentPush[]>([]);

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
    <div className="absolute inset-0 flex flex-col bg-[var(--ct-bg)]">
      <div className="flex items-center gap-0 px-4 py-0 text-[13px] bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] text-[var(--ct-fg)]">
        <div className="flex items-center gap-3 mr-4">
          <strong>Mesh</strong>
          <span className="text-[11px] opacity-70">
            {data.error
              ? `error: ${data.error}`
              : `${data.peers.length} peer${data.peers.length === 1 ? "" : "s"} · ${data.services.length} service${data.services.length === 1 ? "" : "s"}`}
          </span>
          <button
            onClick={() => setRefreshKey((k) => k + 1)}
            className={`text-[11px] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] bg-transparent border-none cursor-pointer ${data.loading ? "animate-spin" : ""}`}
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
              className={activeTab === tab ? TAB_ACTIVE : TAB_INACTIVE}
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
      </div>
    </div>
  );
}
