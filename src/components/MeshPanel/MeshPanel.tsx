import { useEffect, useRef, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import type { MeshTab, Service } from "./types";
import { MeshCanvas } from "./MeshCanvas";
import { ServicesTable } from "./ServicesTable";
import { ServiceDrawer, type DrawerState } from "./ServiceDrawer";
import { AddServiceWizard } from "./AddServiceWizard";
import { useMeshData } from "./useMeshData";

const TAB_BASE =
  "px-3 py-2 text-xs border-b-2 bg-transparent cursor-pointer capitalize";
const TAB_ACTIVE = `${TAB_BASE} border-[var(--ct-accent)] text-[var(--ct-fg)]`;
const TAB_INACTIVE = `${TAB_BASE} border-transparent text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]`;

const TABS: MeshTab[] = ["map", "activity", "settings"];

const CANVAS_HEIGHT = 400;

interface Props {
  api: ApiConfig;
  onClose: () => void;
}

export function MeshPanel({ onClose }: Props) {
  const [activeTab, setActiveTab] = useState<MeshTab>("map");
  const [refreshKey, setRefreshKey] = useState(0);
  const data = useMeshData(refreshKey);
  const mapRef = useRef<HTMLDivElement>(null);
  const [mapWidth, setMapWidth] = useState(800);
  const [drawerState, setDrawerState] = useState<DrawerState>(null);
  const [showAddWizard, setShowAddWizard] = useState<boolean>(false);

  useEffect(() => {
    const el = mapRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const r = el.getBoundingClientRect();
      if (r.width > 0) setMapWidth(r.width);
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
        setActiveTab("settings");
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
            {data.loading
              ? "loading…"
              : data.error
                ? `error: ${data.error}`
                : `${data.peers.length} peer${data.peers.length === 1 ? "" : "s"} · ${data.services.length} service${data.services.length === 1 ? "" : "s"}`}
          </span>
          <button
            onClick={() => setRefreshKey((k) => k + 1)}
            className="text-[11px] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] bg-transparent border-none cursor-pointer"
            title="Refresh"
          >
            ↻
          </button>
        </div>
        <div className="flex items-center gap-0 border-l border-[var(--ct-border)] pl-2">
          {TABS.map((tab) => (
            <button
              key={tab}
              onClick={() => setActiveTab(tab)}
              className={activeTab === tab ? TAB_ACTIVE : TAB_INACTIVE}
            >
              {tab}
            </button>
          ))}
        </div>
        <div className="ml-auto">
          <button
            onClick={() => setShowAddWizard(true)}
            className="text-[11px] text-[var(--ct-accent)] hover:text-[var(--ct-fg)] bg-transparent border border-[var(--ct-border)] hover:border-[var(--ct-accent)] rounded px-2 py-1 cursor-pointer"
            title="Add service"
          >
            + Service
          </button>
        </div>
      </div>
      <div className="relative flex-1 min-h-0 overflow-hidden">
        {activeTab === "map" && (
          <div className="absolute inset-0 flex flex-col">
            <div
              ref={mapRef}
              className="flex-shrink-0"
              style={{ height: CANVAS_HEIGHT }}
            >
              <MeshCanvas
                thisMachine={data.thisMachine}
                peers={data.peers}
                services={data.services}
                recentPushes={[]}
                onPeerClick={handlePeerClick}
                onAddService={() => setShowAddWizard(true)}
                width={mapWidth}
                height={CANVAS_HEIGHT}
              />
            </div>
            <div className="flex-1 min-h-0 overflow-auto">
              <ServicesTable
                services={data.services}
                thisMachineName={data.thisMachine.name}
                onServiceClick={handleServiceClick}
              />
            </div>
          </div>
        )}
        {activeTab === "activity" && (
          <div className="p-4 text-xs text-[var(--ct-muted-fg)]">
            Activity (Phase 12)
          </div>
        )}
        {activeTab === "settings" && (
          <div className="p-4 text-xs text-[var(--ct-muted-fg)]">
            Settings (Phase 9)
          </div>
        )}
        <ServiceDrawer
          state={drawerState}
          services={data.services}
          peers={data.peers}
          onClose={() => setDrawerState(null)}
          onActionDone={() => setRefreshKey((k) => k + 1)}
        />
        {showAddWizard && (
          <AddServiceWizard
            onClose={() => setShowAddWizard(false)}
            onCreated={() => setRefreshKey((k) => k + 1)}
          />
        )}
      </div>
    </div>
  );
}
