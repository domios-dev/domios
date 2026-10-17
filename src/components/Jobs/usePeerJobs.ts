import { useCallback, useEffect, useState } from "react";
import type { TransportClient } from "../../lib/transportClient";
import { daemonTransportForPeer } from "../../lib/daemonBootstrap";
import { readPeerJobs, type PeerJobsData } from "./peerJobsData";

export function usePeerJobs(peerId: string, online: boolean, clientForPeer: (peerId: string) => TransportClient = daemonTransportForPeer) {
  const [revision, setRevision] = useState(0);
  const [state, setState] = useState<{ data: PeerJobsData | null; failed: boolean }>({ data: null, failed: false });
  const refresh = useCallback(() => setRevision((value) => value + 1), []);
  useEffect(() => {
    if (!online) { setState({ data: null, failed: true }); return; }
    const controller = new AbortController();
    setState({ data: null, failed: false });
    const read = async () => readPeerJobs(clientForPeer(peerId), controller.signal);
    void read().then(
      (data) => { if (!controller.signal.aborted) setState({ data, failed: false }); },
      () => { if (!controller.signal.aborted) setState({ data: null, failed: true }); },
    );
    return () => controller.abort();
  }, [peerId, online, revision, clientForPeer]);
  return online ? { ...state, refresh } : { data: null, failed: true, refresh };
}
