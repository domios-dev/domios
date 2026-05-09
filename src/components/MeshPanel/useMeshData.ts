import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { MeshStatus, MeshPeersResponse, PairedPeer } from "../../lib/types";
import { servicesList } from "./meshApi";
import type { Service } from "./types";

export interface PeerStatus {
  name: string;
  ip: string;
  online: boolean;
  access_level: string;
}

export interface MeshData {
  thisMachine: { name: string; ip: string };
  peers: PeerStatus[];
  services: Service[];
  loading: boolean;
  error: string | null;
}

const DEFAULT_DATA: MeshData = {
  thisMachine: { name: "this", ip: "127.0.0.1" },
  peers: [],
  services: [],
  loading: true,
  error: null,
};

export function useMeshData(refreshKey: number): MeshData {
  const [data, setData] = useState<MeshData>(DEFAULT_DATA);

  const loadAll = useCallback(async () => {
    setData((d) => ({ ...d, loading: true }));
    try {
      const local: Service[] = await servicesList();
      const status = await invoke<MeshStatus>("mesh_get_status").catch(
        () => null,
      );
      const meshPeers = await invoke<MeshPeersResponse>("mesh_list_peers").catch(
        () => ({ paired: [], discovered: [], incoming_requests: [] }) as MeshPeersResponse,
      );
      const paired: PeerStatus[] = (meshPeers.paired ?? []).map(
        (p: PairedPeer) => ({
          name: p.instance_name,
          ip: p.addresses[0]?.ip ?? "",
          online: p.online,
          access_level: p.access_level,
        }),
      );

      const remotePromises = paired
        .filter((p) => p.online && p.ip)
        .map(async (p) => {
          try {
            const r = await fetch(`http://${p.ip}:7685/api/services`, {
              signal: AbortSignal.timeout(3000),
            });
            if (!r.ok) return [];
            const arr: Service[] = await r.json();
            return arr.map((s) => ({ ...s, source_peer: p.name }));
          } catch {
            return [];
          }
        });
      const remoteResults = await Promise.all(remotePromises);
      const remote = remoteResults.flat();

      setData({
        thisMachine: {
          name: status?.instance_name ?? "this",
          ip: status?.local_ip ?? status?.tailscale_ip ?? "",
        },
        peers: paired,
        services: [...local, ...remote],
        loading: false,
        error: null,
      });
    } catch (e) {
      setData((d) => ({ ...d, loading: false, error: String(e) }));
    }
  }, []);

  useEffect(() => {
    loadAll();
  }, [loadAll, refreshKey]);

  return data;
}
