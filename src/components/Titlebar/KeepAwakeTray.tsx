import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "../../lib/bridge";
import { listen } from "../../lib/bridge";
import { getCachedConfig, loadConfig, setUiSettings } from "../../lib/clientSettings";
import { Power } from "lucide-react";
import { createPortal } from "react-dom";
import { UI_TRAY_BTN } from "../shared/design";
import { LiquidToggleTrack, useLiquidTravel } from "../shared/LiquidToggle";
import { Segmented } from "../shared/Segmented";

interface KeepAwakeStatus {
  active: boolean;
  display: boolean;
  expires_at: number | null;
}

const PRESETS = [
  { value: "inf", label: "∞", title: "Indefinitely", secs: null },
  { value: "1h", label: "1h", title: "1 hour", secs: 3600 },
  { value: "2h", label: "2h", title: "2 hours", secs: 7200 },
  { value: "4h", label: "4h", title: "4 hours", secs: 14400 },
  { value: "8h", label: "8h", title: "8 hours", secs: 28800 },
  { value: "12h", label: "12h", title: "12 hours", secs: 43200 },
] as const;

type PresetValue = (typeof PRESETS)[number]["value"];

// is shared across clients instead of per-localStorage.
function loadDisplay(): boolean {
  return getCachedConfig()?.ui?.keepawake_display === true;
}

function saveDisplay(v: boolean) {
  void setUiSettings({ keepawake_display: v });
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
  const [picked, setPicked] = useState<PresetValue>("inf");
  const [tick, setTick] = useState(0);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  // Hydrate the display preference from centralized config once it loads.
  useEffect(() => {
    void loadConfig()
      .then((cfg) => setDisplaySleep(cfg.ui?.keepawake_display === true))
      .catch(() => {});
  }, []);

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

  // subscription. Guarded with `cancelled` so StrictMode double-mount and async-race teardowns both unlisten cleanly.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    (async () => {
      try {
        const u = await listen<KeepAwakeStatus>("keepawake-changed", (e) => {
          if (!cancelled) applyStatus(e.payload);
        });
        if (cancelled) { u(); return; }
        unlisten = u;
        const snap = await invoke<KeepAwakeStatus>("keepawake_status");
        if (!cancelled) applyStatus(snap);
      } catch {
        /* backend restarting — next mount will retry */
      }
    })();
    return () => { cancelled = true; if (unlisten) unlisten(); };
  }, [applyStatus]);

  useEffect(() => {
    if (!status.active || status.expires_at === null) return;
    const id = setInterval(() => setTick((t) => t + 1), 500);
    return () => clearInterval(id);
  }, [status.active, status.expires_at]);

  // applies the authoritative payload — a single source of truth avoids flicker and out-of-sync races.
  const start = useCallback(async (secs: number | null) => {
    try {
      await invoke<KeepAwakeStatus>("keepawake_start", {
        durationSecs: secs ?? undefined,
        display: displaySleep,
      });
    } catch (e) {
      console.error("[keepawake]", e);
    }
  }, [displaySleep]);

  const stop = useCallback(async () => {
    await invoke("keepawake_stop").catch(() => {});
  }, []);

  const applyDisplay = useCallback((next: boolean) => {
    setDisplaySleep(next);
    saveDisplay(next);
    if (!status.active) return;
    // side; clamp to ≥ 1s so a near-expiry toggle doesn't collapse into `Duration::from_secs(0)` which the backend would treat as instant expiry. Indefinite sessions stay indefinite.
    let durSecs: number | undefined;
    if (status.expires_at !== null) {
      const remaining = Math.ceil((status.expires_at - Date.now()) / 1000);
      durSecs = Math.max(1, remaining);
    }
    invoke<KeepAwakeStatus>("keepawake_start", { durationSecs: durSecs, display: next })
      .catch(() => {});
  }, [status.active, status.expires_at]);

  const {
    flip: flipDisplay,
    travel: displayTravel,
    finishTravel: finishDisplayTravel,
  } = useLiquidTravel(displaySleep, applyDisplay);

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

  // The strip follows status where it can: a countdown drifts off its preset, so a
  // session begun elsewhere keeps this window's last pick; the header clock rules time.
  useEffect(() => {
    if (status.active && status.expires_at === null) setPicked("inf");
  }, [status.active, status.expires_at]);

  const onPick = useCallback(
    (value: PresetValue) => {
      setPicked(value);
      const preset = PRESETS.find((p) => p.value === value);
      if (preset) void start(preset.secs);
    },
    [start],
  );

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
        type="button"
        data-testid="keepawake-tray"
        onClick={() => setOpen((v) => !v)}
        title={titleText}
        aria-label={titleText}
        aria-pressed={status.active}
        className={
          status.active
            ? "ct-liquid-interactive flex items-center justify-center w-7 h-7 rounded-md text-[var(--ct-accent)] hover:bg-[var(--ct-hover)]"
            : UI_TRAY_BTN
        }
      >
        <Power size={14} />
      </button>

      {open && pos && createPortal(
        <div
          ref={menuRef}
          className="ct-portal-menu fixed w-[260px] border border-[rgba(255,255,255,0.12)] rounded-lg shadow-2xl backdrop-blur-xl overflow-hidden"
          style={{ top: pos.top, right: pos.right, background: "var(--ct-surface, #1e1e2e)" }}
        >
          {/* Header row */}
          <div className="flex items-center h-10 px-3">
            <div className="flex-1 flex items-baseline gap-1.5 min-w-0">
              <span className="text-[12px] font-semibold text-[var(--ct-fg)]">Keep Awake</span>
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
          </div>

          <div className="h-px bg-[rgba(255,255,255,0.08)]" />

          <div className="flex items-center gap-2 px-3 py-2">
            <button
              type="button"
              className="ct-keepawake-off"
              aria-pressed={!status.active}
              title="Off"
              aria-label="Turn keep awake off"
              onClick={() => void stop()}
            >
              ✕
            </button>
            <Segmented
              aria-label="Keep awake duration"
              className={`flex-1${status.active ? "" : " ct-segmented-idle"}`}
              height={26}
              value={picked}
              onChange={onPick}
              options={PRESETS.map((p) => ({ value: p.value, label: p.label, title: p.title }))}
            />
          </div>

          <div className="h-px bg-[rgba(255,255,255,0.08)]" />

          {/* Display sleep toggle */}
          <button
            type="button"
            role="switch"
            aria-checked={displaySleep}
            onClick={flipDisplay}
            className="flex items-center justify-between w-full px-3 py-2 text-[11px] text-[var(--ct-fg)] hover:bg-[rgba(255,255,255,0.06)] transition-colors"
          >
            <span className="flex flex-col items-start">
              <span>Keep display awake</span>
              <span className="text-[9.5px] text-[var(--ct-muted-fg)] mt-0.5">
                {displaySleep ? "Display stays on" : "Display can sleep"}
              </span>
            </span>
            <LiquidToggleTrack
              checked={displaySleep}
              travel={displayTravel}
              onTravelEnd={finishDisplayTravel}
            />
          </button>
        </div>,
        document.body
      )}
    </>
  );
}
