import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "../../lib/bridge";
import type { MeshPeersResponse, DiscoveredPeer, MeshPairingRequest, PairedPeer } from "../../lib/types";
import { UI_BUTTON, UI_INPUT, UI_PANEL, UI_TEXT } from "../shared/design";
import { BusySpinner } from "../shared/BusyState";

const ACTION_BTN_CLASS = `${UI_BUTTON.ghost} ${UI_BUTTON.compact}`;
const PRIMARY_BTN_CLASS = `${UI_BUTTON.accent} ${UI_BUTTON.compact}`;

type ConnectResult = {
  status?: "pending" | "connected";
  existing?: boolean;
};

type ConnectStatus = {
  instanceId: string;
  message: string;
};

export default function AddPeerPanel() {
  const [peers, setPeers] = useState<MeshPeersResponse | null>(null);
  const [connecting, setConnecting] = useState<string | null>(null);
  const [approving, setApproving] = useState<string | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [manualAddr, setManualAddr] = useState("");
  const [showManual, setShowManual] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [connectStatus, setConnectStatus] = useState<ConnectStatus | null>(null);
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
    const start = Date.now();
    try {
      await invoke("mesh_scan_peers");
      await fetchPeers();
    } catch {}
    const elapsed = Date.now() - start;
    if (elapsed < 800) await new Promise((r) => setTimeout(r, 800 - elapsed));
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
    setConnectStatus(null);
    try {
      const result = await invoke<ConnectResult>("mesh_connect", { instanceId });
      if (result.status === "connected") {
        setConnectStatus({ instanceId, message: "Already connected." });
      } else if (result.status === "pending") {
        setConnectStatus({
          instanceId,
          message: result.existing ? "Pairing request is already pending." : "Pairing request pending approval.",
        });
      }
      await fetchPeers();
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
  const paired = peers?.paired ?? [];
  const pairedIds = new Set(paired.map((p) => p.instance_id));
  // Vouched peers are surfaced in the "Paired devices" card as "auto-pairs when online" — drop them from "Discovered nearby" so users see one row, not two.
  const discovered = (peers?.discovered ?? []).filter(
    (d) => d.vouched_by.length === 0 && !pairedIds.has(d.instance_id),
  );

  return (
    <div className="flex flex-col gap-3">
      {meshOnline === false && (
        <div className={`${UI_PANEL.card} bg-[var(--ct-titlebar-bg)] p-2.5 flex items-center justify-between gap-3`}>
          <span className={UI_TEXT.meta}>
            You're offline — broadcasting peers won't show up here.
          </span>
          <button
            onClick={enableMesh}
            className={`${UI_BUTTON.primary} ${UI_BUTTON.compact} flex-shrink-0`}
            >
            Go online
          </button>
        </div>
      )}
      {error && (
        <div role="alert" data-testid="mesh-connect-error" className={`text-[var(--ct-red)] bg-[var(--ct-bg)] border border-[var(--ct-red)] rounded p-2 break-all ${UI_TEXT.meta}`}>
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
            className={`${ACTION_BTN_CLASS} text-[var(--ct-muted)] disabled:opacity-40`}
          >
            {scanning ? (
              <BusySpinner size={10} />
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
              status={connectStatus?.instanceId === peer.instance_id ? connectStatus.message : null}
            />
          ))
        )}
      </Section>

      <div>
        {showManual ? (
          <div className="flex flex-col gap-2">
            <div className={`${UI_TEXT.metaMuted} uppercase`}>Add manually</div>
            <div className="flex items-center gap-2">
              <input
                type="text"
                value={manualAddr}
                onChange={(e) => setManualAddr(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && handleAddManual()}
                placeholder="192.168.1.50:7685"
                className={`${UI_INPUT.mono} flex-1 bg-[var(--ct-titlebar-bg)]`}
                autoFocus
              />
              <button
                onClick={handleAddManual}
                className={PRIMARY_BTN_CLASS}
              >
                Add
              </button>
              <button
                onClick={() => { setShowManual(false); setManualAddr(""); }}
                className={ACTION_BTN_CLASS}
              >
                Cancel
              </button>
            </div>
          </div>
        ) : (
          <button
            onClick={() => setShowManual(true)}
            className={`${UI_BUTTON.accent} ${UI_BUTTON.compact}`}
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
        <div className={`${UI_TEXT.metaMuted} uppercase`}>{label}</div>
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
          <span className="font-medium text-[var(--ct-fg)] truncate">{req.from_instance_name}</span>
          <span className={`${UI_TEXT.meta} ct-text-mono flex-shrink-0`}>{req.from_address}</span>
        </div>
        <div className="flex items-center gap-1.5 flex-shrink-0">
          <button
            onClick={onApprove}
            disabled={approving}
            className={`${UI_BUTTON.primary} ${UI_BUTTON.compact} disabled:opacity-50`}
          >
            {approving ? "Approving…" : "Approve"}
          </button>
          <button
            onClick={onReject}
            disabled={approving}
            className={`${ACTION_BTN_CLASS} disabled:opacity-50`}
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
  status,
}: {
  peer: DiscoveredPeer;
  paired: PairedPeer[];
  isPending: boolean;
  elapsed: number;
  onConnect: () => void;
  connectDisabled: boolean;
  status: string | null;
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
            <span className="text-sm truncate text-[var(--ct-fg)]">{peer.instance_name}</span>
            <span className={`${UI_TEXT.meta} ct-text-mono flex-shrink-0`}>
              {peer.addresses[0]?.ip || ""}
            </span>
            {peer.dev && (
              <span className={`${UI_PANEL.smallPill} bg-[var(--ct-yellow)]/20 text-[var(--ct-yellow)]`}>
                dev
              </span>
            )}
            <span className={`ct-small-pill flex-shrink-0 ${
              hasVouchers
                ? "bg-[var(--ct-green)]/20 text-[var(--ct-green)]"
                : "bg-[var(--ct-border)] text-[var(--ct-muted)]"
            }`}>
              {hasVouchers ? "vouched" : "discovered"}
          </span>
        </div>
        {isPending ? (
          <div className="flex items-center gap-2 flex-shrink-0">
            <BusySpinner size={12} />
            <span className="text-[10px] text-[var(--ct-muted-fg)]">
              Waiting… {elapsed}s
            </span>
          </div>
        ) : (
          <button
            onClick={onConnect}
            disabled={connectDisabled}
            data-testid={`mesh-connect-${peer.instance_id}`}
            className={`${UI_BUTTON.accent} ${UI_BUTTON.compact} disabled:opacity-50 flex-shrink-0`}
          >
            {hasVouchers ? "Pair too?" : hasPeers ? "Add to Mesh" : "Connect"}
          </button>
        )}
      </div>
      {hasVouchers && !isPending && (
        <div className={`mt-1.5 ${UI_TEXT.meta}`}>
          Vouched by {voucherNames.join(", ")}
        </div>
      )}
      {isPending && (
        <div className={`mt-1.5 ${UI_TEXT.metaMuted}`}>
          Accept the pairing request on {peer.instance_name}
        </div>
      )}
      {status && (
        <div role="status" aria-live="polite" data-testid={`mesh-connect-status-${peer.instance_id}`} className={`mt-1.5 ${UI_TEXT.metaMuted}`}>
          {status}
        </div>
      )}
    </div>
  );
}
