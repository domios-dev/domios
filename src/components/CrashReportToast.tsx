import { useEffect, useRef, useState } from "react";
import { invoke } from "../lib/bridge";
import { listen } from "../lib/bridge";
import { useMultiplexer } from "../store/multiplexer";

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
  const previouslyPresented = useRef(loadPresentedCrashKey());
  const { upsertToast, dismissToast } = useMultiplexer();

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

  const payloadKey = payload ? `${payload.dir ?? ""}|${payload.files.join("|")}` : null;
  const latest = useRef({ crash, payload });
  latest.current = { crash, payload };

  useEffect(() => {
    const { crash, payload } = latest.current;
    if (!crash || !crashKey || previouslyPresented.current === crashKey) return;
    if (recovery?.crash_presentation !== "active") { dismissToast(`crash:${crashKey}`); return; }
    persistPresentedCrashKey(crashKey);
    upsertToast({
      id: `crash:${crashKey}`, tab_id: null, workspace_id: null,
      title: crash.supervisor_action.loop_protected ? "Crash recovery stopped" : "Crash recovery recorded",
      body: [
        `${crash.classification.replaceAll("_", " ")} | previous pid ${crash.previous.pid}`,
        `Backoff ${crash.supervisor_action.backoff_ms}ms | count ${crash.supervisor_action.crash_count_in_window}`,
        payload?.files.join(", "),
        crash.work_policy.ambiguous_execution ? `Ambiguous work: ${crash.work_policy.ambiguous_execution.missing_bindings} bindings not restored | ${crash.work_policy.ambiguous_execution.disposition}` : null,
      ].filter(Boolean).join("\n"),
      severity: "Error", level: "failure", lifetime_ms: 15_000, update_only: true,
      local_actions: payload?.dir ? [{ label: "Open folder", run: () => invoke<void>("open_folder", { path: payload.dir }) }] : undefined,
    });
  }, [crashKey, payloadKey, recovery?.crash_presentation, upsertToast, dismissToast]);

  return null;
}
