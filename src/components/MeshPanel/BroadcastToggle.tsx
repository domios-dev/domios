import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Radio } from "lucide-react";

interface MeshStatus {
  broadcast_active?: boolean;
  broadcast_expires_at?: number;
}

interface AppConfigShape { mesh: { enabled: boolean }; [k: string]: unknown }

export function BroadcastToggle() {
  const [active, setActive] = useState(false);
  const [remaining, setRemaining] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [autoEnabledHint, setAutoEnabledHint] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const s = await invoke<MeshStatus>("mesh_get_status");
      setActive(!!s.broadcast_active);
      if (s.broadcast_active && s.broadcast_expires_at) {
        setRemaining(Math.max(0, s.broadcast_expires_at - Math.floor(Date.now() / 1000)));
      } else {
        setRemaining(null);
      }
    } catch {}
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  useEffect(() => {
    if (!active) return;
    const tick = setInterval(() => {
      setRemaining((r) => {
        if (r === null || r <= 1) { refresh(); return r; }
        return r - 1;
      });
    }, 1000);
    return () => clearInterval(tick);
  }, [active, refresh]);

  const toggle = async () => {
    setBusy(true);
    setAutoEnabledHint(false);
    try {
      if (active) {
        await invoke("mesh_stop_broadcast");
      } else {
        // Broadcast requires mesh persistence on. Flip it on first if
        // user is currently offline and surface a hint so they know.
        const cfg = await invoke<AppConfigShape>("get_config");
        if (!cfg.mesh.enabled) {
          await invoke("update_config", {
            config: { ...cfg, mesh: { ...cfg.mesh, enabled: true } },
          });
          setAutoEnabledHint(true);
        }
        await invoke("mesh_start_broadcast");
      }
      await refresh();
    } catch (e) {
      alert(`Broadcast toggle failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col items-end gap-1">
      <button
        onClick={toggle}
        disabled={busy}
        title={active ? "Stop broadcasting (currently visible to other peers)" : "Start broadcasting (be discoverable for 5 min)"}
        className={`inline-flex items-center gap-1.5 text-[11px] px-2 py-1 rounded border cursor-pointer transition-all disabled:opacity-50 ${
          active
            ? "border-[var(--ct-accent)] text-[var(--ct-accent)] bg-[var(--ct-accent)]/10"
            : "border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:border-[var(--ct-accent)]"
        }`}
      >
        <Radio
          size={12}
          className={active ? "text-[var(--ct-accent)] animate-pulse" : ""}
        />
        <span>{active ? formatSeconds(remaining ?? 0) : "Broadcast"}</span>
      </button>
      {autoEnabledHint && (
        <span className="text-[10px] text-[var(--ct-muted)]">
          Mesh online — required for broadcast
        </span>
      )}
    </div>
  );
}

function formatSeconds(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = sec % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}
