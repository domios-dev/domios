import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, listen } from "../../lib/bridge";
import type { MeshStatus, MeshPeersResponse, PairedPeer } from "../../lib/types";
import { DEV_BUILD } from "../../lib/devBuild";
import { notifyMeshPeersLoaded } from "../../lib/localInstanceName";
import type { Service } from "./types";
import { buildHash } from "../../lib/buildStamp";
import {
  diffFrontendCacheRecords,
  frontendCacheRepository,
  frontendCacheScope,
  type FrontendCacheRecord,
} from "../../lib/frontendCacheRecords";
import {
  cachedServices,
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
  /** True once a live load or the cache supplied real data; the defaults are placeholders. */
  loaded: boolean;
}

const DEFAULT_DATA: MeshData = {
  thisMachine: { name: "this", ip: "127.0.0.1" },
  peers: [],
  services: [],
  loading: true,
  error: null,
  loaded: false,
};

const CACHE_SCOPE = "codeterm:mesh-tab-cache:v3";
const CACHE_MAX_AGE_MS = 24 * 3600 * 1000;
const CACHE_SCHEMA = 3;

interface CachePayload {
  thisMachine: MeshData["thisMachine"];
  peers: PeerStatus[];
  services: Service[];
  saved_at: number;
}

interface MeshCacheMetadata {
  schemaVersion: number;
  buildHash: string;
  savedAt: number;
}

const meshCacheByScope = new Map<string, CachePayload>();
const meshWriteTicket = new Map<string, number>();

function cacheScope(): string | null {
  try {
    return frontendCacheScope(CACHE_SCOPE);
  } catch {
    return null;
  }
}

function cacheRecords(payload: Pick<MeshData, "thisMachine" | "peers" | "services">): FrontendCacheRecord[] {
  const records: FrontendCacheRecord[] = [{ kind: "machine", id: "local", value: payload.thisMachine }];
  payload.peers.forEach((peer, position) => records.push({ kind: "peer", id: peer.instance_id, value: { position, peer } }));
  payload.services.forEach((service, position) => records.push({ kind: "service", id: `${position}:${service.id}`, value: { position, service } }));
  return records;
}

function orderedMeshRows<T>(records: FrontendCacheRecord[], kind: string, property: string, expectedId: (row: Record<string, unknown>, position: number) => string): T[] | null {
  const rows: Array<{ position: number; value: T }> = [];
  for (const record of records.filter((candidate) => candidate.kind === kind)) {
    if (!record.value || typeof record.value !== "object") return null;
    const wrapper = record.value as Record<string, unknown>;
    const value = wrapper[property];
    if (!Number.isInteger(wrapper.position) || (wrapper.position as number) < 0 || !value || typeof value !== "object") return null;
    const position = wrapper.position as number;
    if (expectedId(value as Record<string, unknown>, position) !== record.id) return null;
    rows.push({ position, value: value as T });
  }
  rows.sort((left, right) => left.position - right.position);
  if (rows.some((row, index) => row.position !== index)) return null;
  return rows.map(({ value }) => value);
}

function meshPayloadFromRecords(records: FrontendCacheRecord[], savedAt: number): CachePayload | null {
  const machines = records.filter((record) => record.kind === "machine");
  if (records.some((record) => !["machine", "peer", "service"].includes(record.kind)) ||
    machines.length !== 1 || machines[0].id !== "local" || !machines[0].value || typeof machines[0].value !== "object") return null;
  const machine = machines[0].value as CachePayload["thisMachine"];
  if (typeof machine.name !== "string" || typeof machine.ip !== "string") return null;
  const peers = orderedMeshRows<PeerStatus>(records, "peer", "peer", (row) => String(row.instance_id ?? ""));
  const services = orderedMeshRows<Service>(records, "service", "service", (row, position) => `${position}:${String(row.id ?? "")}`);
  if (!peers || !services) return null;
  return { thisMachine: machine as CachePayload["thisMachine"], peers, services, saved_at: savedAt };
}

async function readCache(): Promise<CachePayload | null> {
  const scope = cacheScope();
  if (!scope) return null;
  const writeTicket = meshWriteTicket.get(scope) ?? 0;
  try {
    const snapshot = await frontendCacheRepository.read(scope);
    if (cacheScope() !== scope || meshWriteTicket.get(scope) !== writeTicket) return null;
    if (!snapshot) return null;
    const metadata = snapshot.manifest.metadata as unknown as MeshCacheMetadata;
    if (!metadata || metadata.schemaVersion !== CACHE_SCHEMA || metadata.buildHash !== buildHash() ||
      !Number.isFinite(metadata.savedAt) || metadata.savedAt <= 0 || Date.now() - metadata.savedAt > CACHE_MAX_AGE_MS) {
      await frontendCacheRepository.clear(scope, snapshot.manifest.revision);
      meshCacheByScope.delete(scope);
      return null;
    }
    const payload = meshPayloadFromRecords(snapshot.records, metadata.savedAt);
    if (!payload) return null;
    meshCacheByScope.set(scope, payload);
    return payload;
  } catch {
    return null;
  }
}

async function writeCache(payload: Pick<MeshData, "thisMachine" | "peers" | "services">): Promise<void> {
  const scope = cacheScope();
  if (!scope) return;
  const ticket = (meshWriteTicket.get(scope) ?? 0) + 1;
  meshWriteTicket.set(scope, ticket);
  try {
    let previous = meshCacheByScope.get(scope);
    if (!previous) {
      const snapshot = await frontendCacheRepository.read(scope);
      if (meshWriteTicket.get(scope) !== ticket) return;
      if (snapshot) {
        const metadata = snapshot.manifest.metadata as unknown as MeshCacheMetadata;
        if (metadata.schemaVersion === CACHE_SCHEMA && metadata.buildHash === buildHash()) {
          previous = meshPayloadFromRecords(snapshot.records, metadata.savedAt) ?? undefined;
        }
      }
    }
    if (meshWriteTicket.get(scope) !== ticket) return;
    const nextRecords = cacheRecords(payload);
    if (previous) {
      const previousRecords = cacheRecords(previous);
      const changes = diffFrontendCacheRecords(previousRecords, nextRecords);
      const changed = changes.upserts.length > 0 || changes.deletes.length > 0;
      if (!changed) {
        meshCacheByScope.set(scope, { ...payload, saved_at: previous.saved_at });
        return;
      }
    }
    const savedAt = Date.now();
    const result = await frontendCacheRepository.write(scope, nextRecords, {
      schemaVersion: CACHE_SCHEMA,
      buildHash: buildHash(),
      savedAt,
    });
    if (result.refused) {
      meshCacheByScope.delete(scope);
      return;
    }
    meshCacheByScope.set(scope, { ...payload, saved_at: savedAt });
  } catch {
    return;
  }
}

export interface MeshDataResult extends MeshData {
  refresh: () => void;
}

export function useMeshData(refreshKey: number): MeshDataResult {
  const [data, setData] = useState<MeshData>(DEFAULT_DATA);
  const dataRef = useRef(data);
  dataRef.current = data;
  const liveAcceptedRef = useRef(false);
  const refreshFailedRef = useRef(false);

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
    refreshFailedRef.current = false;
    setData((d) => ({ ...d, loading: true }));
    try {
      const localView = await invoke<MeshInventoryView>("mesh_inventory_list");
      const local: Service[] = localView.resources;
      const status = await invoke<MeshStatus>("mesh_get_status").catch(
        () => null,
      );
      // A failed peer list is an error state, never an authoritative "0 peers".
      const meshPeers = await invoke<MeshPeersResponse>("mesh_list_peers");
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
            if (DEV_BUILD) console.debug(`[mesh] services fetch failed for ${p.name}`, e);
            return { peerId: p.instance_id, peerName: p.name, view: null };
          }
        });
      const remoteResults = await Promise.all(remotePromises);
      for (const err of collectPeerErrors(remoteResults)) {
        if (DEV_BUILD) console.debug(`[mesh] peer services refresh failed: ${err}`);
      }

      const previous = dataRef.current;
      const remoteWithCached = remoteResults.map((result) => {
        if (result.view) return result;
        const peer = paired.find((candidate) => candidate.instance_id === result.peerId);
        const lastPeer = previous.peers.find((candidate) => candidate.instance_id === result.peerId);
        if (!peer || !lastPeer?.services_loaded) return result;
        const services = previous.services.filter((service) => service.source_peer_id === result.peerId || service.machine_id === result.peerId);
        return { ...result, view: {
          services, stale: true, fetched_at: lastPeer.services_fetched_at ?? null, peer_online: peer.online,
          error: null, freshness: "stale" as const, partial_error: null,
        } };
      });
      const remoteById = new Map(remoteWithCached.map((r) => [r.peerId, r] as const));
      const peersWithServiceState = paired.map((p) => {
        const r = remoteById.get(p.instance_id);
        if (!r) return p;
        return {
          ...p,
          services_loaded: !!r.view,
          services_stale: r.view?.stale ?? true,
          services_error: r.view?.error ?? r.view?.partial_error ?? (r.view ? null : "services fetch failed"),
          services_fetched_at: r.view?.fetched_at ?? null,
        };
      });

      const next = {
        thisMachine: {
          name: status?.instance_name ?? "this",
          ip: status?.local_ip ?? status?.tailscale_ip ?? "",
        },
        peers: peersWithServiceState,
        services: mergeServicesView(local, remoteWithCached),
      };
      liveAcceptedRef.current = true;
      setData({ ...next, loading: false, error: null, loaded: true });
      void writeCache(next);
      notifyMeshPeersLoaded();
    } catch (e) {
      refreshFailedRef.current = true;
      if (DEV_BUILD) console.debug("[mesh] local inventory fetch failed", e);
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
    void readCache().then((cached) => {
      if (!active || !cached || liveAcceptedRef.current) return;
      setData((current) => ({
        thisMachine: cached.thisMachine,
        peers: cached.peers.map((peer) => ({ ...peer, services_stale: true })),
        services: cachedServices(cached.services),
        loading: !refreshFailedRef.current,
        error: refreshFailedRef.current ? current.error : null,
        loaded: true,
      }));
    });
    return () => { active = false; };
  }, []);

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
