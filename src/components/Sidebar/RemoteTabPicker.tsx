import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { ArrowLeft, Laptop, LoaderCircle, X } from "lucide-react";
import { invoke } from "../../lib/bridge";
import { BusySpinner } from "../shared/BusyState";
import type { MeshPeersResponse, PairedPeer, Tab } from "../../lib/types";
import type { RemotePeerTabs } from "../../lib/remoteTabPortal";
import { groupRemoteTabs, peerTopologyOf, type PeerTopology } from "./remoteTabPickerModel";

interface RemoteTabPickerProps {
  onAttach: (peer: RemotePeerTabs, tab: Tab) => void;
  onClose: () => void;
}

export function RemoteTabPicker({ onAttach, onClose }: RemoteTabPickerProps) {
  const [peers, setPeers] = useState<PairedPeer[]>([]);
  const [selectedPeer, setSelectedPeer] = useState<PairedPeer | null>(null);
  const [remote, setRemote] = useState<RemotePeerTabs | null>(null);
  const [topology, setTopology] = useState<PeerTopology | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.code === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    void invoke<MeshPeersResponse>("mesh_list_peers")
      .then((value) => setPeers(value.paired.filter((peer) => peer.online)))
      .catch((cause) => setError(String(cause)))
      .finally(() => setLoading(false));
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  const groups = useMemo(() => groupRemoteTabs(remote?.tabs ?? [], topology), [remote, topology]);

  const choosePeer = (peer: PairedPeer) => {
    setSelectedPeer(peer);
    setRemote(null);
    setTopology(null);
    setError(null);
    setLoading(true);
    // Names are a refinement: a peer that refuses its state still lists its tabs, grouped without names.
    const names = invoke<unknown>("mesh_peer_mux_state", { peerId: peer.instance_id })
      .then(peerTopologyOf, () => null);
    void Promise.all([invoke<RemotePeerTabs>("mesh_peer_tabs_list", { peerId: peer.instance_id }), names])
      .then(([tabs, named]) => { setRemote(tabs); setTopology(named); })
      .catch((cause) => setError(String(cause)))
      .finally(() => setLoading(false));
  };

  return createPortal(
    <div
      className="fixed inset-0 z-[1000] flex items-center justify-center bg-black/55 p-5 backdrop-blur-sm"
      onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="remote-tab-picker-title"
        className="relative w-full max-w-md overflow-hidden rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)] shadow-2xl"
      >
        <header className="ct-jobs-header flex h-12 items-center gap-2 px-3">
          {selectedPeer && (
            <button type="button" aria-label="Back to machines" onClick={() => { setSelectedPeer(null); setRemote(null); setError(null); }} className="flex h-7 w-7 items-center justify-center rounded hover:bg-[var(--ct-hover)]">
              <ArrowLeft size={14} />
            </button>
          )}
          <h2 id="remote-tab-picker-title" className="min-w-0 flex-1 truncate text-[13px] font-medium text-[var(--ct-fg)]">
            {selectedPeer ? selectedPeer.instance_name : "Open remote Tab"}
          </h2>
          <button type="button" aria-label="Close remote Tab picker" onClick={onClose} className="flex h-7 w-7 items-center justify-center rounded hover:bg-[var(--ct-hover)]">
            <X size={14} />
          </button>
        </header>
        <div className="max-h-[min(60vh,420px)] min-h-40 overflow-y-auto p-2">
          {loading && <div role="status" aria-label="Loading" className="flex h-32 items-center justify-center text-[var(--ct-muted-fg)]"><BusySpinner size={17} icon={LoaderCircle} /></div>}
          {!loading && error && <InlineErrorNotice error={error} operation="loading remote tabs" blocking />}
          {!loading && !error && !selectedPeer && peers.map((peer) => (
            <button key={peer.instance_id} type="button" onClick={() => choosePeer(peer)} className="flex w-full items-center gap-3 rounded px-3 py-2 text-left hover:bg-[var(--ct-hover)]">
              <Laptop size={15} className="text-[var(--ct-muted-fg)]" />
              <span className="truncate text-[13px] text-[var(--ct-fg)]">{peer.instance_name}</span>
            </button>
          ))}
          {!loading && !error && !selectedPeer && peers.length === 0 && <div className="px-3 py-8 text-center text-[12px] text-[var(--ct-muted-fg)]">No online machines</div>}
          {!loading && !error && selectedPeer && groups.map((workspace) => (
            <section key={workspace.id ?? ""} aria-label={workspace.name} className="mb-1">
              <h3 className="px-3 pb-1 pt-2 text-[11px] font-medium uppercase tracking-wide text-[var(--ct-muted-fg)]">{workspace.name}</h3>
              {workspace.teams.map((team) => (
                <div key={team.id ?? ""} role="group" aria-label={team.name ?? `${workspace.name} Tabs`}>
                  {team.name && <div className="truncate px-3 pb-0.5 pt-1 text-[11px] text-[var(--ct-muted-fg)]">{team.name}</div>}
                  {team.rows.map((row) => (
                    <button key={row.tab.id} type="button" onClick={() => { if (remote) onAttach(remote, row.tab); onClose(); }} className={`flex w-full min-w-0 items-center gap-3 rounded py-2 pr-3 text-left hover:bg-[var(--ct-hover)] ${team.name ? "pl-5" : "pl-3"}`}>
                      <span className="h-2 w-2 shrink-0 rounded-full bg-emerald-500" />
                      <span className="min-w-0 flex-1 truncate text-[13px] text-[var(--ct-fg)]">{row.title}</span>
                      <span className="max-w-[45%] shrink-0 truncate text-[11px] text-[var(--ct-muted-fg)]">{row.kind}</span>
                    </button>
                  ))}
                </div>
              ))}
            </section>
          ))}
          {!loading && !error && selectedPeer && groups.length === 0 && <div className="px-3 py-8 text-center text-[12px] text-[var(--ct-muted-fg)]">No available Tabs</div>}
        </div>
      </div>
    </div>,
    document.body,
  );
}
