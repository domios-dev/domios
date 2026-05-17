import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { MeshStatus, MeshPeersResponse, PairedPeer, DiscoveredPeer } from "../../lib/types";
import { servicesList } from "./meshApi";
import type { Service } from "./types";

export interface PeerStatus {
  instance_id: string;
  name: string;
  ip: string;
  api_port: number;
  online: boolean;
  access_level: string;
  last_seen: string | null;
  /** Vouched-but-not-yet-paired. UI shows in Paired section with "auto-pairs when online". */
  pending?: boolean;
  /** instance_names of paired peers that vouched for this one (only for pending). */
  vouched_by_names?: string[];
}

const LAST_SEEN_GRACE_MS = 90_000;

function withinGrace(lastSeen: string | null | undefined): boolean {
  if (!lastSeen) return false;
  const t = Date.parse(lastSeen);
  return !isNaN(t) && Date.now() - t <= LAST_SEEN_GRACE_MS;
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

const CACHE_KEY = "mesh-tab-cache-v1";
const CACHE_MAX_AGE_MS = 24 * 3600 * 1000;

interface CachePayload {
  thisMachine: MeshData["thisMachine"];
  peers: PeerStatus[];
  services: Service[];
  saved_at: number;
}

function readCache(): CachePayload | null {
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (!raw) return null;
    const p = JSON.parse(raw) as CachePayload;
    if (!p.saved_at || Date.now() - p.saved_at > CACHE_MAX_AGE_MS) return null;
    return p;
  } catch { return null; }
}

function writeCache(d: Pick<MeshData, "thisMachine" | "peers" | "services">) {
  try {
    const payload: CachePayload = { ...d, saved_at: Date.now() };
    localStorage.setItem(CACHE_KEY, JSON.stringify(payload));
  } catch {}
}

export interface MeshDataResult extends MeshData {
  refresh: () => void;
}

export function useMeshData(refreshKey: number): MeshDataResult {
  const [data, setData] = useState<MeshData>(() => {
    const cached = readCache();
    if (!cached) return DEFAULT_DATA;
    return {
      thisMachine: cached.thisMachine,
      peers: cached.peers,
      services: cached.services,
      loading: true,
      error: null,
    };
  });

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
          instance_id: p.instance_id,
          name: p.instance_name,
          ip: p.addresses[0]?.ip ?? "",
          api_port: p.api_port,
          online: p.online,
          access_level: p.access_level,
          last_seen: p.last_seen,
        }),
      );

      // Vouched-but-not-yet-paired peers: render in the Paired section with
      // "will auto-pair when online" so users don't see a manual button.
      const pairedIds = new Set(paired.map((p) => p.instance_id));
      const pairedNamesById = new Map(paired.map((p) => [p.instance_id, p.name] as const));
      const pending: PeerStatus[] = (meshPeers.discovered ?? [])
        .filter((d: DiscoveredPeer) => d.vouched_by.length > 0 && !pairedIds.has(d.instance_id))
        .map((d: DiscoveredPeer) => ({
          instance_id: d.instance_id,
          name: d.instance_name,
          ip: d.addresses[0]?.ip ?? "",
          api_port: d.api_port,
          online: false,
          access_level: "pending",
          last_seen: null,
          pending: true,
          vouched_by_names: d.vouched_by
            .map((id) => pairedNamesById.get(id))
            .filter((n): n is string => Boolean(n)),
        }));

      const remotePromises = paired
        .filter((p) => p.ip && (p.online || withinGrace(p.last_seen)))
        .map(async (p) => {
          try {
            const arr = await invoke<Service[]>("mesh_peer_services_list", {
              instanceId: p.instance_id,
            });
            return arr.map((s) => ({ ...s, source_peer: p.name }));
          } catch {
            return [];
          }
        });
      const remoteResults = await Promise.all(remotePromises);
      const remote = remoteResults.flat();

      const next = {
        thisMachine: {
          name: status?.instance_name ?? "this",
          ip: status?.local_ip ?? status?.tailscale_ip ?? "",
        },
        peers: [...paired, ...pending],
        services: [...local, ...remote],
      };
      writeCache(next);
      setData({ ...next, loading: false, error: null });
    } catch (e) {
      setData((d) => ({ ...d, loading: false, error: String(e) }));
    }
  }, []);

  useEffect(() => {
    loadAll();
    const handle = setInterval(() => { loadAll(); }, 5000);
    return () => clearInterval(handle);
  }, [loadAll, refreshKey]);

  return { ...data, refresh: loadAll };
}
