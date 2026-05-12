import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Radio } from "lucide-react";

interface MeshStatus {
  broadcast_active?: boolean;
  broadcast_expires_at?: number;
}

export function BroadcastToggle() {
  const [active, setActive] = useState(false);
  const [remaining, setRemaining] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

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
    try {
      if (active) await invoke("mesh_stop_broadcast");
      else await invoke("mesh_start_broadcast");
      await refresh();
    } catch (e) {
      alert(`Broadcast toggle failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  return (
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
  );
}

function formatSeconds(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = sec % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}
