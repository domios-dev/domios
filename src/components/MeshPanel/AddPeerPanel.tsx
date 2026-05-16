import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { MeshPeersResponse, DiscoveredPeer, MeshPairingRequest, PairedPeer } from "../../lib/types";

export default function AddPeerPanel() {
  const [peers, setPeers] = useState<MeshPeersResponse | null>(null);
  const [connecting, setConnecting] = useState<string | null>(null);
  const [approving, setApproving] = useState<string | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [manualAddr, setManualAddr] = useState("");
  const [showManual, setShowManual] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const elapsedRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const [meshOnline, setMeshOnline] = useState<boolean | null>(null);
  const [scanning, setScanning] = useState(false);

  const fetchPeers = useCallback(async () => {
    try {
      const p = await invoke<MeshPeersResponse>("mesh_list_peers");
      setPeers(p);
    } catch {}
  }, []);

  const fetchMeshState = useCallback(async () => {
    try {
      const cfg = await invoke<{ mesh: { enabled: boolean } }>("get_config");
      setMeshOnline(cfg.mesh.enabled);
    } catch {}
  }, []);

  const enableMesh = useCallback(async () => {
    try {
      const cfg = await invoke<{ mesh: { enabled: boolean }; [k: string]: unknown }>("get_config");
      await invoke("update_config", {
        config: { ...cfg, mesh: { ...cfg.mesh, enabled: true } },
      });
      setMeshOnline(true);
    } catch (e) {
      console.error("Enable mesh failed:", e);
    }
  }, []);

  const handleScan = useCallback(async () => {
    setScanning(true);
    try {
      await invoke("mesh_scan_peers");
      await fetchPeers();
    } catch {}
    setScanning(false);
  }, [fetchPeers]);

  useEffect(() => {
    fetchPeers();
    fetchMeshState();
    pollRef.current = setInterval(() => {
      fetchPeers();
      fetchMeshState();
    }, 2000);
    return () => {
      if (pollRef.current) clearInterval(pollRef.current);
    };
  }, [fetchPeers, fetchMeshState]);

  useEffect(() => {
    if (!connecting) {
      if (elapsedRef.current) { clearInterval(elapsedRef.current); elapsedRef.current = null; }
      setElapsed(0);
      return;
    }
    setElapsed(0);
    elapsedRef.current = setInterval(() => setElapsed((n) => n + 1), 1000);
    return () => { if (elapsedRef.current) clearInterval(elapsedRef.current); };
  }, [connecting]);

  const handleConnect = async (instanceId: string) => {
    if (connecting) return;
    setConnecting(instanceId);
    setError(null);
    try {
      await invoke("mesh_connect", { instanceId });
    } catch (e) {
      setError(String(e));
    } finally {
      setConnecting(null);
    }
  };

  const handleApprove = async (requestId: string) => {
    setApproving(requestId);
    setError(null);
    try {
      await invoke("mesh_approve_pairing", { requestId });
      await fetchPeers();
    } catch (e) {
      setError(String(e));
    } finally {
      setApproving(null);
    }
  };

  const handleReject = async (requestId: string) => {
    try {
      await invoke("mesh_reject_pairing", { requestId });
      await fetchPeers();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleAddManual = async () => {
    const parts = manualAddr.split(":");
    const address = parts[0];
    const port = parseInt(parts[1] || "7685", 10);
    if (!address) return;
    setError(null);
    try {
      await invoke("mesh_add_manual", { address, port });
      setManualAddr("");
      setShowManual(false);
      await fetchPeers();
    } catch (e) {
      setError(String(e));
    }
  };

  const incoming = peers?.incoming_requests ?? [];
  const discovered = peers?.discovered ?? [];
  const paired = peers?.paired ?? [];

  return (
    <div className="flex flex-col gap-3">
      {meshOnline === false && (
        <div className="text-[11px] bg-[var(--ct-titlebar-bg)] border border-[var(--ct-border)] rounded p-2.5 flex items-center justify-between gap-3">
          <span className="text-[var(--ct-muted-fg)]">
            You're offline — broadcasting peers won't show up here.
          </span>
          <button
            onClick={enableMesh}
            className="text-[10px] uppercase tracking-wider px-2.5 py-1 rounded border border-[var(--ct-accent)]/40 text-[var(--ct-accent)] hover:bg-[var(--ct-accent)]/10 transition-colors flex-shrink-0"
          >
            Go online
          </button>
        </div>
      )}
      {error && (
        <div className="text-[10px] text-[var(--ct-red)] bg-[var(--ct-bg)] border border-[var(--ct-red)] rounded p-2 break-all">
          {error}
        </div>
      )}

      {incoming.length > 0 && (
        <Section label="Incoming requests">
          {incoming.map((req) => (
            <IncomingRequestRow
              key={req.request_id}
              req={req}
              approving={approving === req.request_id}
              onApprove={() => handleApprove(req.request_id)}
              onReject={() => handleReject(req.request_id)}
            />
          ))}
        </Section>
      )}

      <Section
        label="Discovered nearby"
        action={
          <button
            onClick={handleScan}
            disabled={scanning}
            title="Scan for peers now"
            className="text-[9px] uppercase tracking-wider text-[var(--ct-muted)] hover:text-[var(--ct-fg)] transition-colors disabled:opacity-40 flex items-center gap-1"
          >
            {scanning ? (
              <span className="w-2.5 h-2.5 border border-[var(--ct-accent)] border-t-transparent rounded-full animate-spin" />
            ) : (
              <svg className="w-3 h-3" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5">
                <path d="M13.5 2.5A6.5 6.5 0 1 1 2.5 8" strokeLinecap="round" />
                <path d="M13.5 2.5V6h-3.5" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            )}
            {scanning ? "Scanning…" : "Refresh"}
          </button>
        }
      >
        {discovered.length === 0 ? (
          <div className="p-3 text-[10px] text-[var(--ct-muted)] italic">
            No peers found nearby
          </div>
        ) : (
          discovered.map((peer) => (
            <DiscoveredPeerRow
              key={peer.instance_id}
              peer={peer}
              paired={paired}
              isPending={connecting === peer.instance_id}
              elapsed={connecting === peer.instance_id ? elapsed : 0}
              onConnect={() => handleConnect(peer.instance_id)}
              connectDisabled={connecting !== null}
            />
          ))
        )}
      </Section>

      <div>
        {showManual ? (
          <div className="flex flex-col gap-2">
            <div className="text-[10px] text-[var(--ct-muted)] uppercase tracking-wide">Add manually</div>
            <div className="flex items-center gap-2">
              <input
                type="text"
                value={manualAddr}
                onChange={(e) => setManualAddr(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && handleAddManual()}
                placeholder="192.168.1.50:7685"
                className="flex-1 bg-[var(--ct-titlebar-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none font-mono"
                autoFocus
              />
              <button
                onClick={handleAddManual}
                className="text-[10px] px-3 py-1.5 rounded bg-[var(--ct-accent)] text-white hover:brightness-110 transition-all"
              >
                Add
              </button>
              <button
                onClick={() => { setShowManual(false); setManualAddr(""); }}
                className="text-[10px] px-2 py-1.5 rounded text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]"
              >
                Cancel
              </button>
            </div>
          </div>
        ) : (
          <button
            onClick={() => setShowManual(true)}
            className="text-[10px] text-[var(--ct-accent)] hover:underline"
          >
            + Add peer manually…
          </button>
        )}
      </div>
    </div>
  );
}

function Section({ label, action, children }: { label: string; action?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <div className="text-[10px] text-[var(--ct-muted)] uppercase tracking-wide">{label}</div>
        {action}
      </div>
      <div className="rounded border border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] divide-y divide-[var(--ct-border)]">
        {children}
      </div>
    </div>
  );
}

function IncomingRequestRow({
  req,
  approving,
  onApprove,
  onReject,
}: {
  req: MeshPairingRequest;
  approving: boolean;
  onApprove: () => void;
  onReject: () => void;
}) {
  return (
    <div className="p-3 flex items-center justify-between gap-2">
      <div className="flex items-center gap-2 min-w-0">
        <div className="w-2 h-2 rounded-full bg-[var(--ct-accent)] animate-pulse flex-shrink-0" />
        <span className="text-xs font-medium text-[var(--ct-fg)] truncate">{req.from_instance_name}</span>
        <span className="text-[9px] text-[var(--ct-muted)] font-mono flex-shrink-0">{req.from_address}</span>
      </div>
      <div className="flex items-center gap-1.5 flex-shrink-0">
        <button
          onClick={onApprove}
          disabled={approving}
          className="text-[10px] px-3 py-1 rounded bg-[var(--ct-accent)] text-white hover:brightness-110 transition-all disabled:opacity-50"
        >
          {approving ? "Approving…" : "Approve"}
        </button>
        <button
          onClick={onReject}
          disabled={approving}
          className="text-[10px] px-2 py-1 rounded text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors disabled:opacity-50"
        >
          Reject
        </button>
      </div>
    </div>
  );
}

function DiscoveredPeerRow({
  peer,
  paired,
  isPending,
  elapsed,
  onConnect,
  connectDisabled,
}: {
  peer: DiscoveredPeer;
  paired: PairedPeer[];
  isPending: boolean;
  elapsed: number;
  onConnect: () => void;
  connectDisabled: boolean;
}) {
  const voucherNames = (peer.vouched_by ?? [])
    .map((id) => paired.find((p) => p.instance_id === id)?.instance_name)
    .filter((n): n is string => Boolean(n));
  const hasVouchers = voucherNames.length > 0;
  const hasPeers = paired.length > 0;

  return (
    <div className="p-3">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0 flex-wrap">
          <div className="w-2 h-2 rounded-full border border-[var(--ct-muted)] flex-shrink-0" />
          <span className="text-xs text-[var(--ct-fg)] truncate">{peer.instance_name}</span>
          <span className="text-[10px] text-[var(--ct-muted)] font-mono flex-shrink-0">
            {peer.addresses[0]?.ip || ""}
          </span>
          {peer.dev && (
            <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-yellow)]/20 text-[var(--ct-yellow)] flex-shrink-0">
              dev
            </span>
          )}
          <span className={`text-[9px] px-1.5 py-0.5 rounded-full flex-shrink-0 ${
            hasVouchers
              ? "bg-[var(--ct-green)]/20 text-[var(--ct-green)]"
              : "bg-[var(--ct-border)] text-[var(--ct-muted)]"
          }`}>
            {hasVouchers ? "vouched" : "discovered"}
          </span>
        </div>
        {isPending ? (
          <div className="flex items-center gap-2 flex-shrink-0">
            <div className="w-3 h-3 border-2 border-[var(--ct-accent)] border-t-transparent rounded-full animate-spin" />
            <span className="text-[10px] text-[var(--ct-muted-fg)]">
              Waiting… {elapsed}s
            </span>
          </div>
        ) : (
          <button
            onClick={onConnect}
            disabled={connectDisabled}
            className="text-[10px] px-3 py-1 rounded bg-[var(--ct-accent)]/20 text-[var(--ct-accent)] hover:bg-[var(--ct-accent)]/30 transition-all disabled:opacity-50 flex-shrink-0"
          >
            {hasVouchers ? "Pair too?" : hasPeers ? "Add to Mesh" : "Connect"}
          </button>
        )}
      </div>
      {hasVouchers && !isPending && (
        <div className="mt-1.5 text-[10px] text-[var(--ct-muted-fg)]">
          Vouched by {voucherNames.join(", ")}
        </div>
      )}
      {isPending && (
        <div className="mt-1.5 text-[10px] text-[var(--ct-muted)]">
          Accept the pairing request on {peer.instance_name}
        </div>
      )}
    </div>
  );
}
