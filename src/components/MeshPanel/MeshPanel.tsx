import { useEffect, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import type { MeshTab } from "./types";

const TAB_BASE =
  "px-3 py-2 text-xs border-b-2 bg-transparent cursor-pointer capitalize";
const TAB_ACTIVE = `${TAB_BASE} border-[var(--ct-accent)] text-[var(--ct-fg)]`;
const TAB_INACTIVE = `${TAB_BASE} border-transparent text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]`;

const TABS: MeshTab[] = ["map", "activity", "settings"];

interface Props {
  api: ApiConfig;
  onClose: () => void;
}

export function MeshPanel({ onClose }: Props) {
  const [activeTab, setActiveTab] = useState<MeshTab>("map");

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

  return (
    <div className="absolute inset-0 flex flex-col bg-[var(--ct-bg)]">
      <div className="flex items-center gap-0 px-4 py-0 text-[13px] bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] text-[var(--ct-fg)]">
        <div className="flex items-center gap-3 mr-4">
          <strong>Mesh</strong>
          <span className="text-[11px] opacity-70">loading…</span>
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
      </div>
      <div className="relative flex-1 min-h-0 overflow-auto">
        {activeTab === "map" && (
          <div className="p-4 text-xs text-[var(--ct-muted-fg)]">
            Map view (Phase 5 Task 5.2)
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
      </div>
    </div>
  );
}
