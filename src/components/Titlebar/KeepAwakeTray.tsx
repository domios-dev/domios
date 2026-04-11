import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Power } from "lucide-react";
import { createPortal } from "react-dom";

interface KeepAwakeStatus {
  active: boolean;
  display: boolean;
  expires_at: number | null;
}

const PRESETS: { label: string; title: string; secs: number | null }[] = [
  { label: "∞", title: "Indefinitely", secs: null },
  { label: "1h", title: "1 hour", secs: 3600 },
  { label: "2h", title: "2 hours", secs: 7200 },
  { label: "4h", title: "4 hours", secs: 14400 },
  { label: "8h", title: "8 hours", secs: 28800 },
  { label: "12h", title: "12 hours", secs: 43200 },
];

const DISPLAY_KEY = "keepawake.displaySleep";

function loadDisplay(): boolean {
  try { return localStorage.getItem(DISPLAY_KEY) === "1"; } catch { return false; }
}

function saveDisplay(v: boolean) {
  try { localStorage.setItem(DISPLAY_KEY, v ? "1" : "0"); } catch {}
}

function clockParts(expiresAt: number): { h: number; m: number } | null {
  const diff = expiresAt - Date.now();
  if (diff <= 0) return null;
  const totalMins = Math.ceil(diff / 60000);
  return { h: Math.floor(totalMins / 60), m: totalMins % 60 };
}

export function KeepAwakeTray() {
  const [status, setStatus] = useState<KeepAwakeStatus>({ active: false, display: false, expires_at: null });
  const [open, setOpen] = useState(false);
  const [displaySleep, setDisplaySleep] = useState<boolean>(loadDisplay);
  const [tick, setTick] = useState(0);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  // Apply a status update from any source (event, invoke response, menu open).
  const applyStatus = useCallback((s: KeepAwakeStatus) => {
    setStatus((prev) => {
      if (
        prev.active === s.active &&
        prev.display === s.display &&
        prev.expires_at === s.expires_at
      ) {
        return prev;
      }
      return s;
    });
    if (s.active) setDisplaySleep(s.display);
  }, []);

  // Mount: fetch current status + subscribe to backend-emitted state changes.
  // The backend emits "keepawake-changed" on start, stop, and natural expiry
  // (from the platform thread), so the tray never needs to poll.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    invoke<KeepAwakeStatus>("keepawake_status").then(applyStatus).catch(() => {});
    listen<KeepAwakeStatus>("keepawake-changed", (e) => applyStatus(e.payload))
      .then((u) => { unlisten = u; })
      .catch(() => {});
    return () => { if (unlisten) unlisten(); };
  }, [applyStatus]);

  // Heartbeat: 500ms tick drives blinking colon + minute countdown when timed
  useEffect(() => {
    if (!status.active || status.expires_at === null) return;
    const id = setInterval(() => setTick((t) => t + 1), 500);
    return () => clearInterval(id);
  }, [status.active, status.expires_at]);

  const start = useCallback(async (secs: number | null) => {
    try {
      const s = await invoke<KeepAwakeStatus>("keepawake_start", {
        durationSecs: secs ?? undefined,
        display: displaySleep,
      });
      setStatus(s);
      setDisplaySleep(s.display);
    } catch (e) {
      console.error("[keepawake]", e);
    }
  }, [displaySleep]);

  const stop = useCallback(async () => {
    await invoke("keepawake_stop").catch(() => {});
    setStatus({ active: false, display: false, expires_at: null });
  }, []);

  const toggleDisplay = useCallback(() => {
    const next = !displaySleep;
    setDisplaySleep(next);
    saveDisplay(next);
    if (status.active) {
      const durSecs = status.expires_at
        ? Math.max(0, Math.round((status.expires_at - Date.now()) / 1000))
        : undefined;
      invoke<KeepAwakeStatus>("keepawake_start", { durationSecs: durSecs, display: next })
        .then((s) => { setStatus(s); setDisplaySleep(s.display); })
        .catch(() => {});
    }
  }, [displaySleep, status.active, status.expires_at]);

  // Outside click + Escape close
  useEffect(() => {
    if (!open) return;
    const click = (e: MouseEvent) => {
      if (
        menuRef.current && !menuRef.current.contains(e.target as Node) &&
        triggerRef.current && !triggerRef.current.contains(e.target as Node)
      ) setOpen(false);
    };
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("mousedown", click, true);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("mousedown", click, true);
      document.removeEventListener("keydown", key);
    };
  }, [open]);

  const pos = (() => {
    if (!open || !triggerRef.current) return null;
    const rect = triggerRef.current.getBoundingClientRect();
    return { top: rect.bottom + 4, right: window.innerWidth - rect.right };
  })();

  const timed = status.active && status.expires_at !== null;
  const time = timed ? clockParts(status.expires_at!) : null;
  const colonOn = tick % 2 === 0;
  const titleText = status.active
    ? status.expires_at === null
      ? "Keep Awake: Indefinitely"
      : time
        ? `Keep Awake: ${time.h}:${String(time.m).padStart(2, "0")}`
        : "Keep Awake: expiring…"
    : "Keep Awake";

  return (
    <>
      <button
        ref={triggerRef}
        onClick={() => setOpen((v) => !v)}
        title={titleText}
        className={`flex items-center justify-center w-7 h-7 rounded-md transition-colors ${
          status.active
            ? "text-[var(--ct-accent)] hover:bg-[rgba(255,255,255,0.08)]"
            : "text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
        }`}
      >
        <Power size={14} />
      </button>

      {open && pos && createPortal(
        <div
          ref={menuRef}
          className="fixed z-50 w-[260px] border border-[rgba(255,255,255,0.12)] rounded-lg shadow-2xl backdrop-blur-xl overflow-hidden"
          style={{ top: pos.top, right: pos.right, background: "var(--ct-surface, #1e1e2e)" }}
        >
          {/* Header row */}
          <div className="flex items-center h-10 px-3">
            <div className="flex-1 flex items-baseline gap-1.5 min-w-0">
              <span className={`text-[11px] font-medium ${status.active ? "text-[var(--ct-fg)]" : "text-[var(--ct-muted)]"}`}>
                Keep Awake
              </span>
              {status.active && (
                <>
                  <span className="text-[11px] text-[var(--ct-muted)]">—</span>
                  <span className="font-mono text-[13px] font-semibold tabular-nums leading-none text-[var(--ct-accent)]">
                    {status.expires_at === null ? "∞" : time ? (
                      <>
                        {time.h}
                        <span style={{ opacity: colonOn ? 1 : 0.25, transition: "opacity 120ms" }}>:</span>
                        {String(time.m).padStart(2, "0")}
                      </>
                    ) : "0:00"}
                  </span>
                </>
              )}
            </div>
            {status.active && (
              <button
                onClick={(e) => { e.stopPropagation(); stop(); }}
                title="End session"
                className="ml-2 px-2 py-0.5 text-[10px] rounded-full border border-[rgba(255,255,255,0.18)] text-[var(--ct-muted)] hover:border-[var(--ct-red,#ef4444)] hover:text-[var(--ct-red,#ef4444)] hover:bg-[rgba(239,68,68,0.12)] transition-colors"
              >
                End
              </button>
            )}
          </div>

          <div className="h-px bg-[rgba(255,255,255,0.08)]" />

          {/* Preset chips row */}
          <div className="flex items-center gap-1 px-3 py-2">
            {PRESETS.map((p) => (
              <button
                key={p.label}
                onClick={() => start(p.secs)}
                title={p.title}
                className="flex-1 px-1 py-1 text-[11px] rounded-md text-[var(--ct-fg)] border border-[rgba(255,255,255,0.10)] hover:border-[var(--ct-accent)] hover:text-[var(--ct-accent)] hover:bg-[rgba(125,200,255,0.08)] transition-colors"
              >
                {p.label}
              </button>
            ))}
          </div>

          <div className="h-px bg-[rgba(255,255,255,0.08)]" />

          {/* Display sleep toggle */}
          <button
            onClick={toggleDisplay}
            className="flex items-center justify-between w-full px-3 py-2 text-[11px] text-[var(--ct-fg)] hover:bg-[rgba(255,255,255,0.06)] transition-colors"
          >
            <span className="flex flex-col items-start">
              <span>Keep display awake</span>
              <span className="text-[9.5px] text-[var(--ct-muted)] mt-0.5">
                {displaySleep ? "Display stays on" : "Display can sleep"}
              </span>
            </span>
            <span
              className="relative inline-block w-7 h-4 rounded-full transition-colors flex-shrink-0"
              style={{ background: displaySleep ? "var(--ct-accent, #7dc8ff)" : "rgba(255,255,255,0.16)" }}
            >
              <span
                className="absolute top-0.5 w-3 h-3 rounded-full bg-white shadow transition-all"
                style={{ left: displaySleep ? "calc(100% - 14px)" : "2px" }}
              />
            </span>
          </button>
        </div>,
        document.body
      )}
    </>
  );
}
