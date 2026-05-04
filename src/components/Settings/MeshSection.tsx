import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AppConfig, MeshPeersResponse, MeshStatus } from "../../lib/types";
import { Toggle, CopyRow } from "./shared";

export function MeshSection({ config, onUpdate, apiEnabled }: { config: AppConfig; onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void; apiEnabled: boolean }) {
  const [status, setStatus] = useState<MeshStatus | null>(null);
  const [peers, setPeers] = useState<MeshPeersResponse | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [connecting, setConnecting] = useState<string | null>(null);
  const [pendingTimers, setPendingTimers] = useState<Record<string, number>>({});
  const [manualAddr, setManualAddr] = useState("");
  const [showManual, setShowManual] = useState(false);
  const [showHelp, setShowHelp] = useState(false);
  const [exiting, setExiting] = useState(false);
  const [approving, setApproving] = useState<string | null>(null);
  const [approveError, setApproveError] = useState<string | null>(null);
  const [broadcastRemaining, setBroadcastRemaining] = useState<number | null>(null);
  const pendingTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const broadcastTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchData = useCallback(() => {
    invoke<MeshStatus>("mesh_get_status").then(setStatus).catch(() => {});
    invoke<MeshPeersResponse>("mesh_list_peers").then(setPeers).catch(() => {});
  }, []);

  useEffect(() => {
    fetchData();
    const unlisten = listen("mesh-state-changed", () => fetchData());
    return () => { unlisten.then((f) => f()); };
  }, [fetchData]);

  useEffect(() => {
    if (!config.mesh?.enabled) return;
    const interval = setInterval(fetchData, 15000);
    return () => clearInterval(interval);
  }, [config.mesh?.enabled, fetchData]);

  // Pending connection timer — counts up from when connect was initiated
  useEffect(() => {
    if (!connecting) {
      if (pendingTimerRef.current) { clearInterval(pendingTimerRef.current); pendingTimerRef.current = null; }
      return;
    }
    setPendingTimers((prev) => ({ ...prev, [connecting]: 0 }));
    pendingTimerRef.current = setInterval(() => {
      setPendingTimers((prev) => {
        const id = connecting;
        if (!id) return prev;
        return { ...prev, [id]: (prev[id] ?? 0) + 1 };
      });
    }, 1000);
    return () => { if (pendingTimerRef.current) clearInterval(pendingTimerRef.current); };
  }, [connecting]);

  // Broadcast countdown timer
  useEffect(() => {
    if (broadcastTimerRef.current) { clearInterval(broadcastTimerRef.current); broadcastTimerRef.current = null; }
    if (!status?.broadcast_active || !status.broadcast_expires_at) {
      setBroadcastRemaining(null);
      return;
    }
    const tick = () => {
      const remaining = Math.max(0, status.broadcast_expires_at! - Math.floor(Date.now() / 1000));
      setBroadcastRemaining(remaining);
      if (remaining <= 0) {
        setBroadcastRemaining(null);
        fetchData();
      }
    };
    tick();
    broadcastTimerRef.current = setInterval(tick, 1000);
    return () => { if (broadcastTimerRef.current) clearInterval(broadcastTimerRef.current); };
  }, [status?.broadcast_active, status?.broadcast_expires_at, fetchData]);

  const handleBroadcast = useCallback(async () => {
    if (status?.broadcast_active) {
      await invoke("mesh_stop_broadcast").catch(console.error);
    } else {
      await invoke("mesh_start_broadcast").catch(console.error);
    }
    fetchData();
  }, [status?.broadcast_active, fetchData]);

  const handleRefresh = useCallback(async () => {
    setRefreshing(true);
    try { await invoke("mesh_refresh_discovery"); fetchData(); } catch {}
    setTimeout(() => setRefreshing(false), 1000);
  }, [fetchData]);

  const handleConnect = useCallback(async (instanceId: string) => {
    if (connecting) return;
    setConnecting(instanceId);
    try { await invoke("mesh_connect", { instanceId }); } catch (e) { console.error("Mesh connect failed:", e); }
    setConnecting(null);
  }, [connecting]);

  const handleApprove = useCallback(async (requestId: string) => {
    setApproving(requestId);
    setApproveError(null);
    try {
      await invoke("mesh_approve_pairing", { requestId });
    } catch (e: any) {
      const msg = typeof e === "string" ? e : e?.message || "Failed";
      setApproveError(msg);
      console.error("Mesh approve failed:", e);
    } finally {
      setApproving(null);
    }
  }, []);

  const handleReject = useCallback(async (requestId: string) => {
    try { await invoke("mesh_reject_pairing", { requestId }); } catch (e) { console.error("Mesh reject failed:", e); }
  }, []);

  const handleDisconnect = useCallback(async (instanceId: string) => {
    try { await invoke("mesh_disconnect", { instanceId }); } catch (e) { console.error("Mesh disconnect failed:", e); }
  }, []);

  const handleAddManual = useCallback(async () => {
    const parts = manualAddr.split(":");
    const address = parts[0];
    const port = parseInt(parts[1] || "7685", 10);
    if (!address) return;
    try {
      await invoke("mesh_add_manual", { address, port });
      setManualAddr(""); setShowManual(false);
    } catch (e) { console.error("Mesh manual add failed:", e); }
  }, [manualAddr]);

  const handleSetAccess = useCallback(async (instanceId: string, level: string) => {
    try { await invoke("mesh_set_access", { instanceId, accessLevel: level }); } catch (e) { console.error("Set access failed:", e); }
  }, []);

  const handleExit = useCallback(async () => {
    setExiting(true);
    try { await invoke("mesh_exit"); } catch (e) { console.error("Exit mesh failed:", e); }
    setExiting(false);
  }, []);

  const hasPeers = (peers?.paired.length ?? 0) > 0;

  const formatTimeAgo = (iso: string | null) => {
    if (!iso) return "";
    const diff = Date.now() - new Date(iso).getTime();
    const mins = Math.floor(diff / 60000);
    if (mins < 1) return "just now";
    if (mins < 60) return `${mins}m ago`;
    const hrs = Math.floor(mins / 60);
    if (hrs < 24) return `${hrs}h ago`;
    return `${Math.floor(hrs / 24)}d ago`;
  };

  const formatSeconds = (s: number) => {
    const m = Math.floor(s / 60);
    const sec = s % 60;
    return m > 0 ? `${m}m ${sec}s` : `${sec}s`;
  };

  return (
    <div className="space-y-2.5">
      {/* Header */}
      <div className="flex items-center gap-1.5 mb-2.5">
        <h3 className="text-[10px] font-medium uppercase tracking-[0.06em] text-[var(--ct-muted)]">Mesh</h3>
        <button onClick={() => setShowHelp(!showHelp)}
          className="w-3.5 h-3.5 rounded-full border border-[var(--ct-muted)] text-[var(--ct-muted)] text-[8px] flex items-center justify-center hover:text-[var(--ct-fg)] hover:border-[var(--ct-fg)] transition-colors">
          ?
        </button>
      </div>
      {showHelp && (
        <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/80 p-3 mb-2.5 text-[10px] text-[var(--ct-muted-fg)] space-y-1.5">
          <p><strong className="text-[var(--ct-fg)]">Mesh Networking</strong> connects CodeTerm instances across machines. Agents discover peers, delegate tasks, and run commands remotely.</p>
          <p><strong className="text-[var(--ct-fg)]">Security:</strong> Ed25519 signed requests. Manual approval for pairing. Paired peers are trusted. Unpair to revoke access.</p>
        </div>
      )}

      {/* Identity block: PC Name, toggles */}
      <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-2.5">
        {/* Device Name */}
        <div className="flex items-center justify-between text-xs">
          <span className="text-[var(--ct-accent)]">Device</span>
          <input
            type="text"
            value={config.mesh?.instance_name ?? ""}
            onChange={(e) => onUpdate("mesh", { instance_name: e.target.value })}
            className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-2 py-0.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none w-48 text-right"
            placeholder="Device name"
          />
        </div>
        <Toggle
          label="Enable mesh networking"
          checked={config.mesh?.enabled ?? false}
          onChange={(v) => {
            onUpdate("mesh", { enabled: v });
            if (v) {
              if (!apiEnabled) { onUpdate("api" as any, { enabled: true }); }
              setTimeout(() => invoke("mesh_start_discovery").catch(console.error), 500);
            }
          }}
        />
        {config.mesh?.enabled && (
          <div className="flex items-center justify-between text-xs">
            <div className="flex items-center gap-2">
              <span className="text-[var(--ct-muted-fg)]">Broadcast</span>
              {broadcastRemaining !== null && (
                <span className="text-[10px] font-mono text-[var(--ct-accent)]">{formatSeconds(broadcastRemaining)}</span>
              )}
            </div>
            <button onClick={handleBroadcast}
              className={`text-[10px] px-3 py-1 rounded transition-all ${
                status?.broadcast_active
                  ? "bg-[var(--ct-red)]/20 text-[var(--ct-red)] hover:bg-[var(--ct-red)]/30"
                  : "bg-[var(--ct-accent)]/20 text-[var(--ct-accent)] hover:bg-[var(--ct-accent)]/30"
              }`}>
              {status?.broadcast_active ? "Stop" : "Start"}
            </button>
          </div>
        )}
      </div>

      {/* Exit Mesh button */}
      {config.mesh?.enabled && hasPeers && (
        <div className="flex justify-end">
          <button onClick={handleExit} disabled={exiting}
            title="Leave the mesh"
            className="text-[10px] px-3 py-1 rounded border border-[var(--ct-red)]/30 text-[var(--ct-red)]/70 hover:text-[var(--ct-red)] hover:border-[var(--ct-red)] transition-colors disabled:opacity-30">
            {exiting ? "Exiting..." : "Exit Mesh"}
          </button>
        </div>
      )}

      {/* Devices in Mesh — unified section: paired + incoming + discovered */}
      {config.mesh?.enabled && (
        <div className="space-y-2.5">
          <div className="flex items-center justify-between mb-0.5">
            <h3 className="text-[10px] font-medium uppercase tracking-[0.06em] text-[var(--ct-muted)]">Devices in Mesh</h3>
            <button onClick={handleRefresh} disabled={refreshing}
              className="text-[10px] px-2 py-0.5 rounded text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors disabled:opacity-50">
              {refreshing ? "Scanning..." : "Refresh"}
            </button>
          </div>

          <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 divide-y divide-[var(--ct-border)]">
            {/* Paired peers */}
            {peers?.paired.map((peer) => (
              <div key={peer.instance_id} className="p-3 space-y-1.5">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <div className={`w-2 h-2 rounded-full ${peer.online ? "bg-[var(--ct-green)]" : "bg-[var(--ct-muted)]"}`} />
                    <span className="text-xs font-medium text-[var(--ct-fg)]">{peer.instance_name}</span>
                    <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-green)]/20 text-[var(--ct-green)]">paired</span>
                  </div>
                  <span className="text-[10px] text-[var(--ct-muted)] font-mono">{peer.addresses[0]?.ip || ""}</span>
                </div>
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <span className="text-[10px] text-[var(--ct-muted)]">
                      {peer.online ? "Online" : "Offline"} · Paired {formatTimeAgo(peer.paired_at)}
                    </span>
                    <select
                      value={peer.access_level}
                      onChange={(e) => handleSetAccess(peer.instance_id, e.target.value)}
                      className="text-[10px] bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-1.5 py-0.5 text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
                    >
                      <option value="full">Full Access</option>
                      <option value="read_only">Read Only</option>
                      <option value="agent_isolated">Isolated</option>
                    </select>
                  </div>
                  <button onClick={() => handleDisconnect(peer.instance_id)}
                    className="text-[10px] px-2 py-0.5 rounded text-[var(--ct-red)]/70 hover:text-[var(--ct-red)] hover:bg-[var(--ct-red)]/10 transition-colors">
                    Disconnect
                  </button>
                </div>
              </div>
            ))}

            {/* Incoming pairing requests */}
            {peers?.incoming_requests.map((req) => (
              <div key={req.request_id} className="p-3 space-y-1.5">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <div className="w-2 h-2 rounded-full bg-[var(--ct-accent)] animate-pulse" />
                    <span className="text-xs font-medium text-[var(--ct-fg)]">{req.from_instance_name}</span>
                    <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-accent)]/20 text-[var(--ct-accent)]">wants to join</span>
                  </div>
                  <span className="text-[10px] text-[var(--ct-muted)] font-mono">{req.from_address}</span>
                </div>
                <div className="flex items-center gap-2">
                  <button onClick={() => handleApprove(req.request_id)}
                    disabled={approving === req.request_id}
                    className="text-[10px] px-3 py-1 rounded bg-[var(--ct-accent)] text-white hover:brightness-110 transition-all disabled:opacity-50">
                    {approving === req.request_id ? "Approving..." : "Approve"}
                  </button>
                  <button onClick={() => handleReject(req.request_id)}
                    disabled={approving === req.request_id}
                    className="text-[10px] px-3 py-1 rounded text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors disabled:opacity-50">
                    Reject
                  </button>
                  {approveError && approving === null && (
                    <span className="text-[10px] text-[var(--ct-red)]">{approveError}</span>
                  )}
                </div>
              </div>
            ))}

            {/* Discovered peers — "Add to Mesh" type */}
            {peers?.discovered.map((peer) => {
              const isPending = connecting === peer.instance_id;
              const elapsed = pendingTimers[peer.instance_id] ?? 0;
              return (
                <div key={peer.instance_id} className="p-3">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <div className="w-2 h-2 rounded-full border border-[var(--ct-muted)]" />
                      <span className="text-xs text-[var(--ct-muted-fg)]">{peer.instance_name}</span>
                      <span className="text-[10px] text-[var(--ct-muted)] font-mono">{peer.addresses[0]?.ip || ""}</span>
                      {peer.dev && (
                        <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-yellow)]/20 text-[var(--ct-yellow)]">dev</span>
                      )}
                      <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-border)] text-[var(--ct-muted)]">discovered</span>
                    </div>
                    {isPending ? (
                      <div className="flex items-center gap-2">
                        <div className="w-3 h-3 border-2 border-[var(--ct-accent)] border-t-transparent rounded-full animate-spin" />
                        <span className="text-[10px] text-[var(--ct-muted-fg)]">Waiting... {formatSeconds(elapsed)}</span>
                      </div>
                    ) : (
                      <button onClick={() => handleConnect(peer.instance_id)}
                        disabled={connecting !== null}
                        className="text-[10px] px-3 py-1 rounded bg-[var(--ct-accent)]/20 text-[var(--ct-accent)] hover:bg-[var(--ct-accent)]/30 transition-all disabled:opacity-50">
                        {hasPeers ? "Add to Mesh" : "Connect"}
                      </button>
                    )}
                  </div>
                  {isPending && (
                    <div className="mt-1.5 text-[10px] text-[var(--ct-muted)]">
                      Accept the pairing request on {peer.instance_name}
                    </div>
                  )}
                </div>
              );
            })}

            {/* Empty state */}
            {(!peers || (peers.paired.length === 0 && peers.discovered.length === 0 && peers.incoming_requests.length === 0)) && (
              <div className="p-4 text-center text-xs text-[var(--ct-muted)]">
                No devices found. Enable auto-discover or add manually.
              </div>
            )}
          </div>

          {/* Info + Manual add */}
          <div className="space-y-2">
            {status && (
              <div className="space-y-1">
                <CopyRow label="Instance ID" value={status.instance_id} />
                {status.local_ip && <CopyRow label="IP" value={`${status.local_ip}:${status.api_port}`} />}
                {status.tailscale_ip && <CopyRow label="Tailscale" value={`${status.tailscale_ip}:${status.api_port}`} />}
              </div>
            )}
            {showManual ? (
              <div className="flex items-center gap-2">
                <input type="text" value={manualAddr}
                  onChange={(e) => setManualAddr(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleAddManual()}
                  placeholder="192.168.1.50:7685"
                  className="flex-1 bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none font-mono"
                  autoFocus />
                <button onClick={handleAddManual}
                  className="text-[10px] px-3 py-1.5 rounded bg-[var(--ct-accent)] text-white hover:brightness-110 transition-all">Add</button>
                <button onClick={() => { setShowManual(false); setManualAddr(""); }}
                  className="text-[10px] px-2 py-1.5 rounded text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]">Cancel</button>
              </div>
            ) : (
              <button onClick={() => setShowManual(true)}
                className="text-[10px] text-[var(--ct-accent)] hover:underline">
                + Add peer manually...
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
