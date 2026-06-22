import { useCallback, useEffect, useRef, useState } from "react";
import {
  clearRelayTarget,
  localJson,
  relayTarget,
  setRelayTarget,
  selfHost,
} from "../../lib/webApi";

interface PeerAddress {
  ip: string;
  is_tailscale: boolean;
}
interface PairedPeer {
  instance_id: string;
  instance_name: string;
  addresses: PeerAddress[];
  api_port: number;
  online: boolean;
  https_url?: string | null;
}
interface MeshPeers {
  paired: PairedPeer[];
}

/**
 * Web-mirror peer switcher (top-left). Lists the LOCAL daemon's paired mesh
 * peers; selecting one sets a relay target so every request is proxied
 * through the local daemon's mesh relay (the browser stays pointed at its own
 * daemon — no re-point, no peer-scoped token). Selecting "this host" clears
 * the target. A reload gives each switch a clean slate (no stale panes/chat)
 * and reattaches event delivery.
 */
export function WebPeerPicker() {
  const [peers, setPeers] = useState<PairedPeer[]>([]);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);

  const refresh = useCallback(async () => {
    try {
      // Always the LOCAL mesh — never relayed through the active peer.
      const res = await localJson<MeshPeers>("GET", "/api/mesh/peers");
      setPeers(res.paired ?? []);
    } catch {
      // Peer list is a convenience — keep the last known list on errors.
    }
  }, []);

  useEffect(() => {
    void refresh();
    const t = setInterval(() => void refresh(), 15_000);
    return () => clearInterval(t);
  }, [refresh]);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);

  const target = relayTarget();
  const onSelf = !target;
  const label = onSelf ? "this host" : target.name;

  const switchTo = useCallback((peer: PairedPeer | null) => {
    if (peer) {
      setRelayTarget(peer.instance_id, peer.instance_name);
    } else {
      clearRelayTarget();
    }
    // Full reload: clean slate per target — no stale panes/mesh/chat from the
    // previous view, and event delivery reattaches against the new target.
    window.location.reload();
  }, []);

  return (
    <div ref={rootRef} className="relative" data-testid="peer-picker">
      <button
        data-testid="peer-picker-button"
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1.5 min-h-[44px] sm:min-h-0 sm:h-7 px-2.5 rounded text-xs text-[var(--ct-fg)] bg-[var(--ct-hover)] hover:bg-[var(--ct-active)] border border-[var(--ct-border)]"
        title="Switch CodeTerm host"
      >
        <span
          className="w-2 h-2 rounded-full bg-[var(--ct-green,#3fb950)]"
        />
        <span className="max-w-[120px] truncate">{label}</span>
        <span className="opacity-60 text-[9px]">▾</span>
      </button>
      {open && (
        <div
          data-testid="peer-picker-menu"
          className="absolute left-0 top-full mt-1 z-50 min-w-[220px] rounded border border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] shadow-lg py-1"
        >
          <button
            data-testid="peer-picker-self"
            onClick={() => switchTo(null)}
            className={`w-full text-left px-3 min-h-[44px] flex items-center gap-2 text-xs hover:bg-[var(--ct-hover)] ${onSelf ? "text-[var(--ct-accent,#58a6ff)]" : "text-[var(--ct-fg)]"}`}
          >
            <span className="w-2 h-2 rounded-full bg-[var(--ct-green,#3fb950)]" />
            this host ({selfHost().replace(/^https?:\/\//, "")})
          </button>
          {peers.map((p) => {
            const active = target?.id === p.instance_id;
            return (
              <button
                key={p.instance_id}
                data-testid="peer-picker-item"
                data-peer-name={p.instance_name}
                data-peer-online={String(!!p.online)}
                onClick={() => switchTo(p)}
                className={`w-full text-left px-3 min-h-[44px] flex items-center gap-2 text-xs hover:bg-[var(--ct-hover)] disabled:opacity-40 ${active ? "text-[var(--ct-accent,#58a6ff)]" : "text-[var(--ct-fg)]"}`}
                title={p.instance_name}
              >
                <span
                  className={`w-2 h-2 rounded-full ${p.online ? "bg-[var(--ct-green,#3fb950)]" : "bg-[var(--ct-muted,#8b949e)]"}`}
                />
                <span className="flex-1 truncate">{p.instance_name}</span>
                {!p.online && <span className="text-[9px] opacity-60">offline</span>}
              </button>
            );
          })}
          {peers.length === 0 && (
            <div className="px-3 py-2 text-[10px] text-[var(--ct-muted,#8b949e)]">
              no paired peers
            </div>
          )}
        </div>
      )}
    </div>
  );
}
