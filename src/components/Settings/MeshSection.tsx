import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AppConfig, MeshPeersResponse, MeshStatus } from "../../lib/types";
import { Group, Toggle, CopyRow, InfoRow } from "./SettingsPanel";

export function MeshSection({ config, onUpdate, apiEnabled }: { config: AppConfig; onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void; apiEnabled: boolean }) {
  const [status, setStatus] = useState<MeshStatus | null>(null);
  const [peers, setPeers] = useState<MeshPeersResponse | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [connecting, setConnecting] = useState<string | null>(null);
  const [manualAddr, setManualAddr] = useState("");
  const [showManual, setShowManual] = useState(false);
  const [showHelp, setShowHelp] = useState(false);

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

  const handleRefresh = useCallback(async () => {
    setRefreshing(true);
    try { await invoke("mesh_refresh_discovery"); fetchData(); } catch {}
    setTimeout(() => setRefreshing(false), 1000);
  }, [fetchData]);

  const handleConnect = useCallback(async (instanceId: string) => {
    if (connecting) return; // prevent double-click
    setConnecting(instanceId);
    try { await invoke("mesh_connect", { instanceId }); } catch (e) { console.error("Mesh connect failed:", e); }
    setConnecting(null);
  }, [connecting]);

  const handleApprove = useCallback(async (requestId: string) => {
    try { await invoke("mesh_approve_pairing", { requestId }); } catch (e) { console.error("Mesh approve failed:", e); }
  }, []);

  const handleReject = useCallback(async (requestId: string) => {
    try { await invoke("mesh_reject_pairing", { requestId }); } catch (e) { console.error("Mesh reject failed:", e); }
  }, []);

  const handleDisconnect = useCallback(async (instanceId: string) => {
    try { await invoke("mesh_disconnect", { instanceId }); } catch (e) { console.error("Mesh disconnect failed:", e); }
  }, []);

  const handleTransferCrown = useCallback(async (instanceId: string) => {
    try { await invoke("mesh_transfer_crown", { instanceId }); } catch (e) { console.error("Mesh transfer crown failed:", e); }
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

  return (
    <div>
      <div className="flex items-center gap-1.5 mb-2.5">
        <h3 className="text-[10px] font-medium uppercase tracking-[0.06em] text-[var(--ct-muted)]">Mesh</h3>
        <button onClick={() => setShowHelp(!showHelp)}
          className="w-3.5 h-3.5 rounded-full border border-[var(--ct-muted)] text-[var(--ct-muted)] text-[8px] flex items-center justify-center hover:text-[var(--ct-fg)] hover:border-[var(--ct-fg)] transition-colors">
          ?
        </button>
      </div>
      {showHelp && (
        <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/80 p-3 mb-2.5 text-[10px] text-[var(--ct-muted-fg)] space-y-1.5">
          <p><strong className="text-[var(--ct-fg)]">Mesh Networking</strong> connects your CodeTerm instances across machines. Agents can discover peers, delegate tasks, and execute commands remotely.</p>
          <p><strong className="text-[var(--ct-fg)]">How it works:</strong> Enable mesh, discover nearby instances automatically (local network + Tailscale), and connect. One instance becomes the <strong>Crown Holder</strong> — the mesh leader who manages trust levels.</p>
          <p><strong className="text-[var(--ct-fg)]">Security:</strong> All communication uses Ed25519 signed requests. Each instance has a unique keypair. Pairing requires manual approval. Crown holder assigns trust: <em>trusted</em> peers can execute commands, <em>untrusted</em> peers can only receive.</p>
          <p><strong className="text-[var(--ct-fg)]">For agents:</strong> Use <code className="bg-[var(--ct-bg)] px-1 rounded">--mesh PEER</code> flag on any <code className="bg-[var(--ct-bg)] px-1 rounded">codeterm</code> command to run it on a remote peer.</p>
        </div>
      )}
      <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-2.5">
        <div className="flex items-center justify-between text-xs">
          <span className="text-[var(--ct-muted)]">Name</span>
          <input
            type="text"
            value={config.mesh?.instance_name ?? ""}
            onChange={(e) => onUpdate("mesh", { instance_name: e.target.value })}
            className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-2 py-0.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none w-48 text-right"
          />
        </div>
        {status && (
          <div className="flex items-center justify-between text-xs">
            <span className="text-[var(--ct-muted)]">Role</span>
            {status.is_crown ? (
              <span className="text-[var(--ct-yellow)]">Crown Holder</span>
            ) : (
              <span className="text-[var(--ct-muted-fg)]">Member</span>
            )}
          </div>
        )}
        <Toggle
          label="Enable mesh networking"
          checked={config.mesh?.enabled ?? false}
          onChange={(v) => {
            onUpdate("mesh", { enabled: v });
            if (v) {
              // Auto-enable API if not already
              if (!apiEnabled) { onUpdate("api" as any, { enabled: true }); }
              setTimeout(() => invoke("mesh_start_discovery").catch(console.error), 500);
            }
          }}
        />
        {config.mesh?.enabled && (
          <>
            <Toggle
              label="Auto-discover peers"
              checked={config.mesh?.auto_discover ?? true}
              onChange={(v) => onUpdate("mesh", { auto_discover: v })}
            />
            <Toggle
              label="Stealth mode (don't broadcast, listen only)"
              checked={config.mesh?.stealth ?? false}
              onChange={(v) => onUpdate("mesh", { stealth: v })}
            />
          </>
        )}
      </div>

      {config.mesh?.enabled && (
        <div className="space-y-2.5">
          {/* Incoming Pairing Requests — inside mesh section */}
          {peers && peers.incoming_requests.length > 0 && (
            <Group title="Incoming Requests">
              <div className="rounded-lg border border-[var(--ct-accent)]/30 bg-[var(--ct-accent)]/5 divide-y divide-[var(--ct-border)]">
                {peers.incoming_requests.map((req) => (
                  <div key={req.request_id} className="p-3 space-y-1.5">
                    <div className="flex items-center justify-between">
                      <span className="text-xs font-medium text-[var(--ct-fg)]">
                        {req.from_instance_name} wants to join
                      </span>
                      <span className="text-[10px] text-[var(--ct-muted)] font-mono">{req.from_address}</span>
                    </div>
                    <div className="flex items-center gap-2">
                      <button onClick={() => handleApprove(req.request_id)}
                        className="text-[10px] px-3 py-1 rounded bg-[var(--ct-accent)] text-white hover:brightness-110 transition-all">
                        Approve
                      </button>
                      <button onClick={() => handleReject(req.request_id)}
                        className="text-[10px] px-3 py-1 rounded text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors">
                        Reject
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            </Group>
          )}

          {/* Mesh Members */}
          {peers && peers.paired.length > 0 && (
            <Group title={status?.crown_holder ? `Mesh \u00B7 ${peers.paired.find(p => p.instance_id === status.crown_holder)?.instance_name || "unknown"}'s mesh` : "Mesh"}>
              <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 divide-y divide-[var(--ct-border)]">
                {peers.paired.map((peer) => (
                  <div key={peer.instance_id} className="p-3 space-y-1.5">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <div className={`w-2 h-2 rounded-full ${peer.online ? "bg-[var(--ct-green)]" : "bg-[var(--ct-muted)]"}`} />
                        <span className="text-xs font-medium text-[var(--ct-fg)]">{peer.instance_name}</span>
                        {status?.crown_holder === peer.instance_id && (
                          <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-yellow)]/20 text-[var(--ct-yellow)]">crown</span>
                        )}
                      </div>
                      <span className="text-[10px] text-[var(--ct-muted)] font-mono">{peer.addresses[0]?.ip || ""}</span>
                    </div>
                    <div className="flex items-center justify-between">
                      <span className="text-[10px] text-[var(--ct-muted)]">
                        {peer.online ? "Online" : "Offline"} · Paired {formatTimeAgo(peer.paired_at)}
                      </span>
                      <div className="flex items-center gap-1.5">
                        {status?.is_crown && (
                          <select value={peer.trust_level || "untrusted"}
                            onChange={(e) => invoke("mesh_set_trust", { instanceId: peer.instance_id, trustLevel: e.target.value }).catch(console.error)}
                            className="text-[10px] px-1.5 py-0.5 rounded bg-[var(--ct-bg)] border border-[var(--ct-border)] text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none">
                            <option value="untrusted">Untrusted</option>
                            <option value="trusted">Trusted</option>
                          </select>
                        )}
                        {!status?.is_crown && (
                          <span className={`text-[9px] px-1.5 py-0.5 rounded-full ${
                            peer.trust_level === "trusted" ? "bg-[var(--ct-green)]/20 text-[var(--ct-green)]" :
                            "bg-[var(--ct-border)] text-[var(--ct-muted-fg)]"
                          }`}>{peer.trust_level || "untrusted"}</span>
                        )}
                        {status?.is_crown && status?.crown_holder !== peer.instance_id && (
                          <button onClick={() => handleTransferCrown(peer.instance_id)}
                            className="text-[10px] px-2 py-0.5 rounded text-[var(--ct-yellow)]/70 hover:text-[var(--ct-yellow)] hover:bg-[var(--ct-yellow)]/10 transition-colors">
                            Transfer Crown
                          </button>
                        )}
                        <button onClick={() => handleDisconnect(peer.instance_id)}
                          className="text-[10px] px-2 py-0.5 rounded text-[var(--ct-red)]/70 hover:text-[var(--ct-red)] hover:bg-[var(--ct-red)]/10 transition-colors">
                          Disconnect
                        </button>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            </Group>
          )}

          {/* Discovered */}
          <div>
            <div className="flex items-center justify-between mb-2.5">
              <h3 className="text-[10px] font-medium uppercase tracking-[0.06em] text-[var(--ct-muted)]">Discovered</h3>
              <button onClick={handleRefresh} disabled={refreshing}
                className="text-[10px] px-2 py-0.5 rounded text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors disabled:opacity-50">
                {refreshing ? "Scanning..." : "Refresh"}
              </button>
            </div>
            {peers && peers.discovered.length > 0 ? (
              <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 divide-y divide-[var(--ct-border)]">
                {peers.paired.length === 0 && peers.discovered.length > 1 && (
                  <div className="p-3 space-y-2">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <span className="text-xs font-medium text-[var(--ct-fg)]">Mesh Network</span>
                        <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-accent)]/20 text-[var(--ct-accent)]">
                          {peers.discovered.length} peers
                        </span>
                      </div>
                      <button onClick={() => handleConnect(peers.discovered[0].instance_id)}
                        disabled={connecting !== null}
                        className="text-[10px] px-3 py-1 rounded bg-[var(--ct-accent)] text-[var(--ct-fg)] hover:brightness-110 transition-all disabled:opacity-50">
                        {connecting ? "Joining..." : "Join Mesh"}
                      </button>
                    </div>
                    <div className="space-y-1">
                      {peers.discovered.map((peer) => (
                        <div key={peer.instance_id} className="flex items-center gap-2 text-[10px] text-[var(--ct-muted-fg)] pl-2">
                          <div className="w-1.5 h-1.5 rounded-full border border-[var(--ct-muted)]" />
                          <span>{peer.instance_name}</span>
                          <span className="font-mono text-[var(--ct-muted)]">{peer.addresses[0]?.ip || ""}</span>
                          {peer.dev && <span className="text-[var(--ct-yellow)]">dev</span>}
                        </div>
                      ))}
                    </div>
                  </div>
                )}
                {(peers.paired.length > 0 || peers.discovered.length === 1) && peers.discovered.map((peer) => (
                  <div key={peer.instance_id} className="p-3 flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <div className="w-2 h-2 rounded-full border border-[var(--ct-muted)]" />
                      <span className="text-xs text-[var(--ct-fg)]">{peer.instance_name}</span>
                      <span className="text-[10px] text-[var(--ct-muted)] font-mono">{peer.addresses[0]?.ip || ""}</span>
                      {peer.dev && (
                        <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-yellow)]/20 text-[var(--ct-yellow)]">dev</span>
                      )}
                    </div>
                    <button onClick={() => handleConnect(peer.instance_id)}
                      disabled={connecting !== null}
                      className="text-[10px] px-3 py-1 rounded bg-[var(--ct-accent)] text-[var(--ct-fg)] hover:brightness-110 transition-all disabled:opacity-50">
                      {connecting === peer.instance_id ? "Connecting..." : peers.paired.length > 0 ? "Add to Mesh" : "Connect"}
                    </button>
                  </div>
                ))}
              </div>
            ) : (
              <div className="rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 text-center text-xs text-[var(--ct-muted)]">
                No peers discovered yet
              </div>
            )}
          </div>

          {/* Info + Manual add — bottom section */}
          <div className="space-y-2">
            {status && (
              <div className="space-y-1">
                <CopyRow label="Instance ID" value={status.instance_id} />
                {status.local_ip && <InfoRow label="IP" value={status.local_ip} mono />}
                {status.tailscale_ip && <InfoRow label="Tailscale" value={status.tailscale_ip} mono />}
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
