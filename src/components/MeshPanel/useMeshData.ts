import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, listen } from "../../lib/bridge";
import type { MeshStatus, MeshPeersResponse, PairedPeer, DiscoveredPeer } from "../../lib/types";
import type { Service } from "./types";
import {
  collectPeerErrors,
  mergeServicesView,
  type PeerFetchResult,
  type PeerServicesView,
  type MeshInventoryView,
} from "./servicesView";

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
  services_stale?: boolean;
  services_error?: string | null;
  services_fetched_at?: number | null;
  services_loaded?: boolean;
  relay_protocol_version: number;
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

const CACHE_KEY = "mesh-tab-cache-v2";
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

  const inFlightRef = useRef(false);
  const pendingRefreshRef = useRef(false);
  const loadAllRef = useRef<(() => Promise<void>) | null>(null);

  const loadAll = useCallback(async () => {
    // Peer fetches can take several seconds (multi-address walk on a dying peer); don't stack a new cycle on top of a running one.
    if (inFlightRef.current) {
      pendingRefreshRef.current = true;
      return;
    }
    inFlightRef.current = true;
    setData((d) => ({ ...d, loading: true }));
    try {
      const localView = await invoke<MeshInventoryView>("mesh_inventory_list");
      const local: Service[] = localView.resources;
      const status = await invoke<MeshStatus>("mesh_get_status").catch(
        () => null,
      );
      const meshPeers = await invoke<MeshPeersResponse>("mesh_list_peers").catch(
        () => ({ paired: [], discovered: [], incoming_requests: [], local_instance_name: "" }),
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
          relay_protocol_version: p.relay_protocol_version,
        }),
      );

      // Vouched-but-not-yet-paired peers: render in the Paired section with "will auto-pair when online" so users don't see a manual button.
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
          relay_protocol_version: 0,
          pending: true,
          vouched_by_names: d.vouched_by
            .map((id) => pairedNamesById.get(id))
            .filter((n): n is string => Boolean(n)),
        }));

      // (stale-flagged) services instead of vanishing (#119/#127).
      const remotePromises = paired
        .map(async (p): Promise<PeerFetchResult> => {
          try {
            const view = await invoke<PeerServicesView>(
              "mesh_peer_services_list",
              { instanceId: p.instance_id },
            );
            return { peerId: p.instance_id, peerName: p.name, view };
          } catch (e) {
            console.warn(`[mesh] services fetch failed for ${p.name}:`, e);
            return { peerId: p.instance_id, peerName: p.name, view: null };
          }
        });
      const remoteResults = await Promise.all(remotePromises);
      for (const err of collectPeerErrors(remoteResults)) {
        console.warn(`[mesh] peer services: ${err}`);
      }

      const remoteById = new Map(remoteResults.map((r) => [r.peerId, r] as const));
      const peersWithServiceState = paired.map((p) => {
        const r = remoteById.get(p.instance_id);
        if (!r) return p;
        return {
          ...p,
          services_loaded: !!r.view,
          services_stale: r.view?.stale ?? true,
          services_error: r.view?.error ?? (r.view ? null : "services fetch failed"),
          services_fetched_at: r.view?.fetched_at ?? null,
        };
      });

      const next = {
        thisMachine: {
          name: status?.instance_name ?? "this",
          ip: status?.local_ip ?? status?.tailscale_ip ?? "",
        },
        peers: [...peersWithServiceState, ...pending],
        services: mergeServicesView(local, remoteResults),
      };
      writeCache(next);
      setData({ ...next, loading: false, error: null });
    } catch (e) {
      setData((d) => ({ ...d, loading: false, error: String(e) }));
    } finally {
      inFlightRef.current = false;
      if (pendingRefreshRef.current) {
        pendingRefreshRef.current = false;
        void loadAllRef.current?.();
      }
    }
  }, []);
  loadAllRef.current = loadAll;

  useEffect(() => {
    let active = true;
    const unlisteners: Array<() => void> = [];
    const refresh = () => {
      if (active) void loadAll();
    };
    const register = (event: string) => listen(event, refresh)
      .then((unlisten) => {
        if (active) unlisteners.push(unlisten);
        else unlisten();
      })
      .catch(() => undefined);
    refresh();
    void Promise.all([
      register("mesh-state-changed"),
      register("service-health-changed"),
      register("service-pushed"),
    ]).then(refresh);
    return () => {
      active = false;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [loadAll, refreshKey]);

  return { ...data, refresh: loadAll };
}
