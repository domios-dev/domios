import { Activity, X } from "lucide-react";
import { useId } from "react";
import { createPortal } from "react-dom";
import type { ApiConfig } from "../../lib/types";
import type { SupporterStatus } from "../Settings/useDecisionToken";
import { ActivityFeed } from "../Supporter/ActivityFeed";
import { useSupporterResource } from "../Supporter/useSupporterResource";
import { useActivityPanel } from "../Supporter/useActivityPanel";
import { SupporterAllowanceRow } from "../Usage/SupporterAllowanceRow";
import { UI_PANEL, UI_TEXT, UI_TRAY_BTN } from "../shared/design";

export function SupporterActivityTray({ api }: { api: ApiConfig }) {
  const panel = useActivityPanel();
  const id = useId();
  const { data: status } = useSupporterResource<SupporterStatus>(api, "/api/supporter/status");
  if (!status?.connected) return null;
  return (
    <>
      <button
        ref={panel.button}
        type="button"
        className={UI_TRAY_BTN}
        aria-label="Supporter activity"
        title="Supporter activity"
        aria-expanded={panel.open}
        aria-controls={panel.open ? id : undefined}
        aria-haspopup="dialog"
        onClick={panel.toggle}
      >
        <Activity size={14} aria-hidden />
      </button>
      {panel.open && createPortal(
        <div
          id={id}
          ref={panel.panel}
          tabIndex={-1}
          role="dialog"
          aria-labelledby={`${id}-title`}
          style={panel.position}
          className="fixed z-50 flex w-[min(28rem,90vw)] flex-col gap-2 overflow-y-auto rounded-lg border border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] p-3 shadow-2xl outline-none"
        >
          <div className="flex min-w-0 items-start justify-between gap-3 border-b border-[var(--ct-border)] pb-2">
            <div className="min-w-0">
              <p className={UI_TEXT.sectionLabel}>Supporter</p>
              <h2 id={`${id}-title`} className="truncate text-sm font-semibold text-[var(--ct-fg)]">Spinal-cord activity</h2>
            </div>
            <button type="button" className={UI_PANEL.drawerClose} aria-label="Close Supporter activity" onClick={panel.close}>
              <X size={14} aria-hidden="true" />
            </button>
          </div>
          <SupporterAllowanceRow api={api} />
          <ActivityFeed api={api} embedded />
        </div>,
        document.body,
      )}
    </>
  );
}
