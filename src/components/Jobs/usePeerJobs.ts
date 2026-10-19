import { useCallback, useEffect, useRef, useState } from "react";
import { DEV_BUILD } from "../../lib/devBuild";
import type { TransportClient } from "../../lib/transportClient";
import { daemonTransportForPeer } from "../../lib/daemonBootstrap";
import { readPeerJobs, type PeerJobsData } from "./peerJobsData";

export function usePeerJobs(peerId: string, online: boolean, clientForPeer: (peerId: string) => TransportClient = daemonTransportForPeer) {
  const [revision, setRevision] = useState(0);
  const [state, setState] = useState<{ peerId: string | null; data: PeerJobsData | null; failed: boolean; actionFailed: boolean }>({ peerId: null, data: null, failed: false, actionFailed: false });
  const userRefresh = useRef(false);
  const refresh = useCallback(() => { userRefresh.current = true; setRevision((value) => value + 1); }, []);
  const refreshAfterAction = useCallback(() => { userRefresh.current = false; setRevision((value) => value + 1); }, []);
  useEffect(() => {
    if (!online) {
      setState((current) => ({
        peerId,
        data: current.peerId === peerId ? current.data : null,
        failed: true,
        actionFailed: false,
      }));
      return;
    }
    setState((current) => current.peerId === peerId
      ? { ...current, failed: false, actionFailed: false }
      : { peerId, data: null, failed: false, actionFailed: false });
    const actionRefresh = userRefresh.current;
    userRefresh.current = false;
    const controller = new AbortController();
    const read = async () => readPeerJobs(clientForPeer(peerId), controller.signal);
    void read().then(
      (data) => { if (!controller.signal.aborted) setState({ peerId, data, failed: false, actionFailed: false }); },
      (error: unknown) => {
        if (controller.signal.aborted) return;
        if (DEV_BUILD) console.debug("Background peer jobs refresh failed", error);
        setState((current) => {
          const cached = current.peerId === peerId ? current.data : null;
          return { peerId, data: cached, failed: actionRefresh || cached === null, actionFailed: actionRefresh };
        });
      },
    );
    return () => controller.abort();
  }, [peerId, online, revision, clientForPeer]);
  const matchesPeer = state.peerId === peerId;
  return {
    data: matchesPeer ? state.data : null,
    failed: !online || (matchesPeer ? state.failed : false),
    actionFailed: online && matchesPeer && state.actionFailed,
    refresh,
    refreshAfterAction,
  };
}
