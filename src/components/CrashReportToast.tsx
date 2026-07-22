import { useEffect, useState } from "react";
import { invoke } from "../lib/bridge";
import { listen } from "../lib/bridge";
import { AlertTriangle, FolderOpen, X } from "lucide-react";

const PRESENTED_CRASH_KEY = "codeterm.recovery.last-presented-crash";

function loadPresentedCrashKey(): string | null {
  try {
    return window.localStorage.getItem(PRESENTED_CRASH_KEY);
  } catch {
    return null;
  }
}

function persistPresentedCrashKey(key: string) {
  try {
    window.localStorage.setItem(PRESENTED_CRASH_KEY, key);
  } catch {}
}

interface CrashPayload {
  dir: string;
  files: string[];
  recovery?: RecoveryState;
}

interface RecoveryState {
  current: { epoch: string; pid: number; started_at: number; last_healthy_at: number };
  crash_presentation: "active" | "settled";
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
      ambiguous_execution: { missing_bindings: number; disposition: string } | null;
      duplicate_worker_guard: string;
      report_route_guard: string;
    };
  } | null;
}

export function RecoveryStatusBanner() {
  const [payload, setPayload] = useState<CrashPayload | null>(null);
  const [recovery, setRecovery] = useState<RecoveryState | null>(null);
  const [dismissedCrashKey, setDismissedCrashKey] = useState(loadPresentedCrashKey);

  useEffect(() => {
    let cancelled = false;
    let recoveryEventSeen = false;
    const unlisteners: Array<() => void> = [];
    (async () => {
      try {
        const unlistenCrash = await listen<CrashPayload>("crash_report_available", (e) => {
          if (!cancelled) {
            setPayload(e.payload);
            if (e.payload.recovery) {
              recoveryEventSeen = true;
              setRecovery(e.payload.recovery);
            }
          }
        });
        if (cancelled) {
          unlistenCrash();
          return;
        }
        unlisteners.push(unlistenCrash);
        const unlistenRecovery = await listen<RecoveryState>("recovery-state-changed", (e) => {
          if (!cancelled) {
            recoveryEventSeen = true;
            setRecovery(e.payload);
          }
        });
        if (cancelled) {
          unlistenRecovery();
          return;
        }
        unlisteners.push(unlistenRecovery);
      } catch {
        /* event subsystem not ready — frontend reloaded mid-startup */
      }
      invoke<RecoveryState>("get_recovery_state")
        .then((state) => {
          if (!cancelled && !recoveryEventSeen) setRecovery(state);
        })
        .catch(() => {});
    })();
    return () => {
      cancelled = true;
      for (const unlisten of unlisteners) unlisten();
    };
  }, []);

  const crash = recovery?.last_crash;
  const crashKey = crash
    ? `${crash.previous.epoch}:${crash.previous.pid}:${crash.new_epoch}`
    : null;

  useEffect(() => {
    if (!crashKey || recovery?.crash_presentation !== "active" || dismissedCrashKey === crashKey) return;
    persistPresentedCrashKey(crashKey);
  }, [crashKey, dismissedCrashKey, recovery?.crash_presentation]);

  if (!crash || !crashKey || recovery?.crash_presentation !== "active") return null;
  if (dismissedCrashKey === crashKey) return null;

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
    <aside
      data-testid="recovery-status-banner"
      role="status"
      aria-label="Crash recovery status"
      aria-live="polite"
      className="fixed right-3 top-20 z-40 pointer-events-none"
    >
      <div
        className="pointer-events-auto w-[min(340px,calc(100vw-1.5rem))] rounded-lg border px-3 py-3 shadow-2xl ct-float"
        style={{
          background: "color-mix(in srgb, var(--ct-red) 12%, var(--ct-surface))",
          borderColor: "color-mix(in srgb, var(--ct-red) 35%, transparent)",
        }}
      >
        <div className="flex items-start gap-3">
          <div className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-[var(--ct-red)]/15 text-[var(--ct-red)]">
            <AlertTriangle size={16} />
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex items-start justify-between gap-2">
              <div className="min-w-0">
                <div className="truncate text-sm font-medium text-[var(--ct-fg)]">
                  {lead}
                </div>
                <div className="mt-1 text-[11px] leading-relaxed text-[var(--ct-muted-fg)]">
                  {crash.classification.replaceAll("_", " ")} · backoff {crash.supervisor_action.backoff_ms}ms · count {crash.supervisor_action.crash_count_in_window}
                </div>
              </div>
              <button
                type="button"
                onClick={() => {
                  persistPresentedCrashKey(crashKey);
                  setDismissedCrashKey(crashKey);
                }}
                className="flex h-6 w-6 shrink-0 items-center justify-center rounded text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
                title="Dismiss"
                aria-label="Dismiss crash recovery"
              >
                <X size={14} />
              </button>
            </div>
            <div className="mt-1 truncate text-[11px] text-[var(--ct-muted-fg)]">
              {reportText}
            </div>
            {crash.work_policy.ambiguous_execution && (
              <div className="mt-1 text-[11px] leading-relaxed text-[var(--ct-muted-fg)]">
                Ambiguous work: {crash.work_policy.ambiguous_execution.missing_bindings} binding
                {crash.work_policy.ambiguous_execution.missing_bindings === 1 ? "" : "s"} not restored ·{" "}
                {crash.work_policy.ambiguous_execution.disposition}
              </div>
            )}
            {payload?.dir && (
              <button
                onClick={openFolder}
                className="mt-2 inline-flex h-6 items-center gap-1 rounded border px-2 text-[11px]"
                style={{
                  color: "var(--ct-fg)",
                  borderColor: "var(--ct-border)",
                  background: "var(--ct-surface)",
                }}
              >
                <FolderOpen size={12} />
                Open folder
              </button>
            )}
          </div>
        </div>
      </div>
    </aside>
  );
}
