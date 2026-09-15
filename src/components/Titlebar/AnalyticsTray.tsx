import { forwardRef } from "react";
import { ChartColumn } from "lucide-react";
import { UI_TRAY_BTN } from "../shared/design";
import { usePluginSnapshotStore } from "../../lib/pluginSnapshotStore";
import { isAnalyticsAvailable } from "./analyticsTrayLogic";

const OPEN_CLASS =
  "bg-[var(--ct-active)] text-[var(--ct-fg)] after:absolute after:inset-x-[9px] after:bottom-0.5 after:h-0.5 after:rounded-full after:bg-[var(--ct-accent)]";

export const AnalyticsTray = forwardRef<HTMLButtonElement, { open: boolean; onToggle: () => void }>(
  function AnalyticsTray({ open, onToggle }, ref) {
    const { snapshot } = usePluginSnapshotStore();
    if (!isAnalyticsAvailable(snapshot.plugins)) return null;
    return (
      <button
        ref={ref}
        type="button"
        data-testid="titlebar-analytics"
        aria-label="Analytics"
        title="Analytics"
        aria-current={open ? "page" : undefined}
        onClick={onToggle}
        className={`${UI_TRAY_BTN} relative ${open ? OPEN_CLASS : ""}`}
      >
        <ChartColumn size={14} strokeWidth={1.8} aria-hidden />
      </button>
    );
  },
);
