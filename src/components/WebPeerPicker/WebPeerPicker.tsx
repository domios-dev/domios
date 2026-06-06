import { useCallback, useEffect, useRef, useState } from "react";
import {
  apiHost,
  selfHost,
  setApiHost,
  tokenFor,
  webJson,
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
}
interface MeshPeers {
  paired: PairedPeer[];
}

/** Tailscale address preferred (stable across networks), then the rest. */
function peerUrl(peer: PairedPeer): string | null {
  const addr =
    peer.addresses.find((a) => a.is_tailscale) ?? peer.addresses[0];
  return addr ? `http://${addr.ip}:${peer.api_port}` : null;
}

/**
 * Web-mirror peer switcher (top-left). Lists this daemon's paired mesh
 * peers; selecting one points the whole app at that peer's own API
 * (each peer authenticates with its own token — WebGate prompts on
 * first switch). Switching reloads the SPA so no state leaks between
 * hosts; the gate's "Connecting to …" screen covers the transition.
 */
export function WebPeerPicker() {
  const [peers, setPeers] = useState<PairedPeer[]>([]);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);

  const refresh = useCallback(async () => {
    try {
      const res = await webJson<MeshPeers>("GET", "/api/mesh/peers");
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

  const current = apiHost();
  const onSelf = current === selfHost();
  const currentPeer = peers.find((p) => peerUrl(p) === current);
  const label = onSelf
    ? "this host"
    : (currentPeer?.instance_name ?? current.replace(/^https?:\/\//, ""));

  const switchTo = useCallback((target: string | null) => {
    setApiHost(target);
    // Full reload: clean slate per host — no stale panes/mesh/chat data
    // from the previous peer, and the SSE stream reattaches naturally.
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
          className={`w-2 h-2 rounded-full ${onSelf || currentPeer?.online ? "bg-[var(--ct-green,#3fb950)]" : "bg-[var(--ct-muted,#8b949e)]"}`}
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
            const url = peerUrl(p);
            const active = url === current;
            return (
              <button
                key={p.instance_id}
                data-testid="peer-picker-item"
                data-peer-name={p.instance_name}
                data-peer-online={String(!!p.online)}
                disabled={!url}
                onClick={() => url && switchTo(url)}
                className={`w-full text-left px-3 min-h-[44px] flex items-center gap-2 text-xs hover:bg-[var(--ct-hover)] disabled:opacity-40 ${active ? "text-[var(--ct-accent,#58a6ff)]" : "text-[var(--ct-fg)]"}`}
                title={url ?? "no usable address"}
              >
                <span
                  className={`w-2 h-2 rounded-full ${p.online ? "bg-[var(--ct-green,#3fb950)]" : "bg-[var(--ct-muted,#8b949e)]"}`}
                />
                <span className="flex-1 truncate">{p.instance_name}</span>
                {!p.online && <span className="text-[9px] opacity-60">offline</span>}
                {tokenFor(url ?? "") == null && url && (
                  <span className="text-[9px] opacity-60" title="token needed on first switch">
                    🔑
                  </span>
                )}
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
