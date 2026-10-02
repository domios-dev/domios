import { Activity } from "lucide-react";
import { useId } from "react";
import { createPortal } from "react-dom";
import type { ApiConfig } from "../../lib/types";
import { ActivityFeed } from "../Supporter/ActivityFeed";
import { useActivityPanel } from "../Supporter/useActivityPanel";
import { SupporterAllowanceRow } from "../Usage/SupporterAllowanceRow";
import { UI_TRAY_BTN } from "../shared/design";

export function SupporterActivityTray({ api }: { api: ApiConfig }) {
  const panel = useActivityPanel();
  const id = useId();
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
          aria-label="General Supporter activity"
          style={panel.position}
          className="fixed z-50 w-[min(32rem,90vw)] overflow-y-auto rounded border border-[var(--ct-border)] bg-[var(--ct-bg)] p-3 shadow-lg"
        >
          <button type="button" aria-label="Close Supporter activity" onClick={panel.close} className="mb-2">
            Close
          </button>
          <SupporterAllowanceRow api={api} />
          <ActivityFeed api={api} />
        </div>,
        document.body,
      )}
    </>
  );
}
