import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Pill } from "lucide-react";
import { createPortal } from "react-dom";

interface KeepAwakeStatus {
  active: boolean;
  display: boolean;
  expires_at: number | null;
}

const PRESETS: { label: string; secs: number | null }[] = [
  { label: "Indefinitely", secs: null },
  { label: "1 hour", secs: 3600 },
  { label: "2 hours", secs: 7200 },
  { label: "4 hours", secs: 14400 },
  { label: "8 hours", secs: 28800 },
  { label: "12 hours", secs: 43200 },
];

function formatRemaining(expiresAt: number): string {
  const diff = expiresAt - Date.now();
  if (diff <= 0) return "expiring…";
  const mins = Math.floor(diff / 60000);
  if (mins < 60) return `${mins}m left`;
  const hrs = Math.floor(mins / 60);
  const rm = mins % 60;
  return rm > 0 ? `${hrs}h ${rm}m left` : `${hrs}h left`;
}

export function KeepAwakeTray() {
  const [status, setStatus] = useState<KeepAwakeStatus>({ active: false, display: false, expires_at: null });
  const [open, setOpen] = useState(false);
  const [displaySleep, setDisplaySleep] = useState(false);
  const [remaining, setRemaining] = useState("");
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  // Fetch status on mount
  useEffect(() => {
    invoke<KeepAwakeStatus>("keepawake_status").then(setStatus).catch(() => {});
  }, []);

  // Tick remaining timer every 30s when active
  useEffect(() => {
    if (!status.active) { setRemaining(""); return; }
    if (!status.expires_at) { setRemaining("Indefinitely"); return; }
    const update = () => setRemaining(formatRemaining(status.expires_at!));
    update();
    const id = setInterval(update, 30000);
    return () => clearInterval(id);
  }, [status.active, status.expires_at]);

  // Check for natural expiry (timer thread cleared state)
  useEffect(() => {
    if (!status.active) return;
    const id = setInterval(() => {
      invoke<KeepAwakeStatus>("keepawake_status").then((s) => {
        if (!s.active) setStatus(s);
      }).catch(() => {});
    }, 60000);
    return () => clearInterval(id);
  }, [status.active]);

  const start = useCallback(async (secs: number | null) => {
    try {
      const s = await invoke<KeepAwakeStatus>("keepawake_start", {
        durationSecs: secs ?? undefined,
        display: displaySleep,
      });
      setStatus(s);
    } catch (e) {
      console.error("[keepawake]", e);
    }
    setOpen(false);
  }, [displaySleep]);

  const stop = useCallback(async () => {
    await invoke("keepawake_stop").catch(() => {});
    setStatus({ active: false, display: false, expires_at: null });
    setOpen(false);
  }, []);

  // Close on click outside
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (
        menuRef.current && !menuRef.current.contains(e.target as Node) &&
        triggerRef.current && !triggerRef.current.contains(e.target as Node)
      ) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", handler, true);
    return () => document.removeEventListener("mousedown", handler, true);
  }, [open]);

  // Close on Escape
  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [open]);

  const pos = (() => {
    if (!open || !triggerRef.current) return null;
    const rect = triggerRef.current.getBoundingClientRect();
    return { top: rect.bottom + 4, right: window.innerWidth - rect.right };
  })();

  return (
    <>
      <button
        ref={triggerRef}
        onClick={() => setOpen((v) => !v)}
        className={`flex items-center justify-center w-7 h-7 rounded-md transition-colors ${
          status.active
            ? "text-[var(--ct-accent)] hover:bg-[rgba(255,255,255,0.08)]"
            : "text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
        }`}
        title={status.active ? `Keep Awake: ${remaining}` : "Keep Awake"}
      >
        <Pill size={14} />
      </button>

      {open && pos && createPortal(
        <div
          ref={menuRef}
          className="fixed z-50 min-w-[180px] border border-[rgba(255,255,255,0.12)] rounded-md shadow-lg py-1 backdrop-blur-xl"
          style={{ top: pos.top, right: pos.right, background: "var(--ct-surface, #1e1e2e)" }}
        >
          {/* Status line */}
          <div className="px-3 py-1.5 text-[10px] text-[var(--ct-muted)] uppercase tracking-wider">
            {status.active ? remaining : "Keep Awake"}
          </div>

          <div className="h-px bg-[rgba(255,255,255,0.08)] mx-2 my-0.5" />

          {/* Presets */}
          {PRESETS.map((p) => (
            <button
              key={p.label}
              onClick={() => start(p.secs)}
              className={`flex items-center w-full px-3 py-1.5 text-[11px] transition-colors ${
                status.active && (
                  (p.secs === null && status.expires_at === null) ||
                  (p.secs !== null && status.expires_at !== null)
                )
                  ? "text-[var(--ct-accent)]"
                  : "text-[var(--ct-fg)]"
              } hover:bg-[rgba(255,255,255,0.08)]`}
            >
              {p.label}
            </button>
          ))}

          <div className="h-px bg-[rgba(255,255,255,0.08)] mx-2 my-0.5" />

          {/* Display sleep toggle */}
          <button
            onClick={() => {
              const next = !displaySleep;
              setDisplaySleep(next);
              // If already active, re-start with new display setting
              if (status.active) {
                const durSecs = status.expires_at
                  ? Math.max(0, Math.round((status.expires_at - Date.now()) / 1000))
                  : undefined;
                invoke<KeepAwakeStatus>("keepawake_start", {
                  durationSecs: durSecs,
                  display: next,
                }).then(setStatus).catch(() => {});
              }
            }}
            className="flex items-center justify-between w-full px-3 py-1.5 text-[11px] text-[var(--ct-fg)] hover:bg-[rgba(255,255,255,0.08)]"
          >
            <span>Prevent display sleep</span>
            <span className={`text-[10px] ${displaySleep ? "text-[var(--ct-accent)]" : "text-[var(--ct-muted)]"}`}>
              {displaySleep ? "ON" : "OFF"}
            </span>
          </button>

          {/* Turn off */}
          {status.active && (
            <>
              <div className="h-px bg-[rgba(255,255,255,0.08)] mx-2 my-0.5" />
              <button
                onClick={stop}
                className="flex items-center w-full px-3 py-1.5 text-[11px] text-[var(--ct-muted-fg)] hover:bg-[rgba(239,68,68,0.15)] hover:text-[var(--ct-red)]"
              >
                Turn off
              </button>
            </>
          )}
        </div>,
        document.body
      )}
    </>
  );
}
