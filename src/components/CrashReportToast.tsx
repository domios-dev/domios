import { useEffect, useState } from "react";
import { invoke } from "../lib/bridge";
import { listen } from "../lib/bridge";
import { AlertTriangle, FolderOpen } from "lucide-react";

interface CrashPayload {
  dir: string;
  files: string[];
  recovery?: RecoveryState;
}

interface RecoveryState {
  current: { epoch: string; pid: number; started_at: number; last_healthy_at: number };
  last_crash?: {
    classification: string;
    detected_at: number;
    new_epoch: string;
    previous: { epoch: string; pid: number; last_healthy_at: number };
    supervisor_action: {
      backoff_ms: number;
      loop_protected: boolean;
      crash_count_in_window: number;
    };
    work_policy: {
      ambiguous_execution: string;
      duplicate_worker_guard: string;
      report_route_guard: string;
    };
  } | null;
}

export function RecoveryStatusBanner() {
  const [payload, setPayload] = useState<CrashPayload | null>(null);
  const [recovery, setRecovery] = useState<RecoveryState | null>(null);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    (async () => {
      invoke<RecoveryState>("get_recovery_state")
        .then((state) => {
          if (!cancelled) setRecovery(state);
        })
        .catch(() => {});
      try {
        const u = await listen<CrashPayload>("crash_report_available", (e) => {
          if (!cancelled) {
            setPayload(e.payload);
            if (e.payload.recovery) setRecovery(e.payload.recovery);
          }
        });
        if (cancelled) { u(); return; }
        unlisten = u;
      } catch {
        /* event subsystem not ready — frontend reloaded mid-startup */
      }
    })();
    return () => { cancelled = true; if (unlisten) unlisten(); };
  }, []);

  const crash = recovery?.last_crash;
  if (!crash || crash.new_epoch !== recovery?.current.epoch) return null;

  const openFolder = () => {
    if (payload?.dir) invoke("open_folder", { path: payload.dir }).catch(() => {});
  };

  const count = payload?.files.length ?? 0;
  const lead = crash.supervisor_action.loop_protected
    ? "Crash recovery stopped"
    : "Crash recovery recorded";
  const reportText = count > 0
    ? `${payload?.files[0]}${count > 1 ? ` + ${count - 1} more` : ""}`
    : `previous pid ${crash.previous.pid}, epoch ${crash.previous.epoch.slice(0, 8)}`;

  return (
    <section
      data-testid="recovery-status-banner"
      role="status"
      aria-live="polite"
      className="fixed top-0 inset-x-0 z-[9997] pointer-events-auto"
    >
      <div
        className="px-4 py-2 border-b shadow-lg flex items-start justify-center gap-2"
        style={{
          background: "color-mix(in srgb, var(--ct-red) 12%, var(--ct-surface))",
          borderColor: "color-mix(in srgb, var(--ct-red) 35%, transparent)",
        }}
      >
        <AlertTriangle size={14} style={{ color: "var(--ct-red)" }} className="shrink-0 mt-0.5" />
        <div className="flex-1 min-w-0 max-w-3xl">
          <div className="text-[12px] font-semibold" style={{ color: "var(--ct-fg)" }}>
            {lead}
          </div>
          <div className="text-[10px] mt-0.5" style={{ color: "var(--ct-muted-fg)" }}>
            {crash.classification.replaceAll("_", " ")} · backoff {crash.supervisor_action.backoff_ms}ms · count {crash.supervisor_action.crash_count_in_window}
          </div>
          <div className="text-[10px] mt-0.5 truncate" style={{ color: "var(--ct-muted-fg)" }}>
            {reportText}
          </div>
          <div className="text-[10px] mt-1" style={{ color: "var(--ct-muted-fg)" }}>
            Ambiguous work: {crash.work_policy.ambiguous_execution}
          </div>
          {payload?.dir && (
            <button
              onClick={openFolder}
              className="mt-1.5 inline-flex items-center gap-1 text-[10px] px-2 py-0.5 rounded border"
              style={{
                color: "var(--ct-fg)",
                borderColor: "var(--ct-border)",
                background: "var(--ct-surface)",
              }}
            >
              <FolderOpen size={10} />
              Open folder
            </button>
          )}
        </div>
      </div>
    </section>
  );
}
