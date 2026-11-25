import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { daemonTransportForPeer } from "../../lib/daemonBootstrap";
import type { PlanIndexEntry } from "../../components/Canvas/plansApi";
import { remoteTeamName } from "../../components/Sidebar/remoteOwnership";
import { cleanMachineName } from "../../lib/machineIdentity";
import { useAccountMachineNames } from "../../lib/useMeshMachineIdentities";
import { invoke } from "../../lib/bridge";
import type { MeshPeersResponse, MultiplexerState } from "../../lib/types";
import { setWorkspaceBrowseTarget, useWorkspaceBrowseTarget } from "../../lib/workspaceBrowseTarget";
import { pairedGeneralCacheScope, readPairedGeneralCache, writePairedGeneralCache, type CachedPeerGeneral } from "./pairedGeneralCache";
import type { PairedRoster } from "./useMeshPeerProjection";
import {
  pairedAvailability, projectBrowsedWorkspaces, projectPairedGeneral, resolvePeerLastActiveTab,
  type BrowsedWorkspace, type GeneralAvailability, type PairedGeneral, type PeerLastActiveSelection, type PeerOrdinaryTabSummary,
} from "./pairedWorkspaceProjection";

type CacheAccount = string | false | null;

interface GeneralSnapshot {
  account: CacheAccount;
  publicKey: string;
  sessions?: PairedGeneral["sessions"];
  teamNames?: PairedGeneral["teamNames"];
  workspaces?: BrowsedWorkspace[];
  lastActiveTab?: PeerLastActiveSelection | null;
  availability: "online" | "unreachable";
  revision: number;
}

interface PeerRead {
  account: CacheAccount;
  controller: AbortController;
  again: boolean;
}

interface WorkspaceBrowse {
  account?: CacheAccount;
  publicKey?: string;
  target: NonNullable<ReturnType<typeof useWorkspaceBrowseTarget>>;
  availability: GeneralAvailability;
  workspaces: BrowsedWorkspace[];
  synced?: boolean;
}

export interface CachedPlanTitles {
  publicKey: string;
  planTitles: Map<string, string>;
  lookedUpTeamIds: Set<string>;
}

export function unnamedTeamIdsForWorkspaces(workspaces: readonly BrowsedWorkspace[]): string[] {
  const missing = new Set<string>();
  for (const workspace of workspaces) {
    const namedTeams = new Set(workspace.teams.filter(team => remoteTeamName(team.id, team.name)).map(team => team.id));
    for (const { tab } of workspace.tabs) {
      if (tab.team_id && !namedTeams.has(tab.team_id)) missing.add(tab.team_id);
    }
  }
  return [...missing];
}

export function needsPeerPlanTitlesFetch(
  unnamedTeamIds: readonly string[],
  cached: CachedPlanTitles | undefined,
  publicKey: string,
): boolean {
  if (!unnamedTeamIds.length) return false;
  if (!cached || cached.publicKey !== publicKey) return true;
  return unnamedTeamIds.some(teamId => !cached.lookedUpTeamIds.has(teamId));
}

export function usePairedWorkspaces(roster: PairedRoster | null, rosterSettled: boolean, cacheAccount: CacheAccount) {
  const peers = roster?.peers ?? null;
  const accountNames = useAccountMachineNames();
  const rosterRevision = roster?.revision;
  const peerRoster = useRef<MeshPeersResponse | null>(peers);
  peerRoster.current = peers;
  const accountRef = useRef(cacheAccount);
  accountRef.current = cacheAccount;
  const [general, setGeneral] = useState<ReadonlyMap<string, GeneralSnapshot>>(() => new Map());
  const snapshotRevisionRef = useRef(0);
  const switchSyncRef = useRef<{
    target: NonNullable<ReturnType<typeof useWorkspaceBrowseTarget>>;
    baseRevision: number;
  } | null>(null);
  const reads = useRef(new Map<string, PeerRead>());
  const planTitlesCache = useRef(new Map<string, CachedPlanTitles>());
  const cacheScope = cacheAccount ? pairedGeneralCacheScope(cacheAccount) : null;
  const [cached, setCached] = useState<{ scope: string; peers: ReadonlyMap<string, CachedPeerGeneral> }>();
  const cachedPeers = cached?.scope === cacheScope ? cached?.peers : undefined;
  const ready = cacheAccount !== false
    && (!cacheScope || cachedPeers !== undefined)
    && (!cachedPeers?.size || rosterSettled);
  useEffect(() => {
    if (!cacheScope) return;
    let active = true;
    void readPairedGeneralCache(cacheScope).then(peers => {
      if (active) setCached({ scope: cacheScope, peers });
    });
    return () => { active = false; };
  }, [cacheScope]);

  useEffect(() => {
    if (!cacheScope || !cachedPeers || !peers) return;
    const next = new Map<string, CachedPeerGeneral>();
    for (const peer of peers.paired) {
      const fresh = general.get(peer.instance_id);
      const saved = cachedPeers.get(peer.instance_id);
      if (fresh?.account === cacheAccount && fresh.publicKey === peer.public_key && fresh.sessions) {
        if (fresh.sessions.length) next.set(peer.instance_id, { publicKey: peer.public_key, sessions: fresh.sessions });
      } else if (saved?.publicKey === peer.public_key) {
        next.set(peer.instance_id, saved);
      }
    }
    void writePairedGeneralCache(cacheScope, next);
  }, [cacheAccount, cacheScope, cachedPeers, general, peers]);

  const invalidatePeerInventory = useCallback((peerId: string) => {
    const running = reads.current.get(peerId);
    reads.current.delete(peerId);
    planTitlesCache.current.delete(peerId);
    running?.controller.abort();
    const peer = peerRoster.current?.paired.find(item => item.instance_id === peerId);
    if (!peer) return;
    const account = accountRef.current;
    setGeneral(previous => new Map(previous).set(peerId, {
      account,
      publicKey: peer.public_key,
      sessions: [],
      workspaces: [],
      lastActiveTab: null,
      availability: "unreachable",
      revision: ++snapshotRevisionRef.current,
    }));
  }, []);

  const refreshPeerInventory = useCallback((peerId: string) => {
    const roster = peerRoster.current;
    const peer = roster?.paired.find(item => item.instance_id === peerId);
    if (!peer || pairedAvailability(peer, roster?.installations) !== "reconnecting") return;
    const running = reads.current.get(peerId);
    if (running) {
      running.again = true;
      return;
    }
    const account = accountRef.current;
    const read: PeerRead = { account, controller: new AbortController(), again: false };
    const { signal } = read.controller;
    reads.current.set(peerId, read);
    const publish = (next: (previous: GeneralSnapshot | undefined) => GeneralSnapshot | undefined) => {
      const currentRoster = peerRoster.current;
      const current = currentRoster?.paired.find(item => item.instance_id === peerId);
      if (!signal.aborted && accountRef.current === account && current && current.public_key === peer.public_key
        && pairedAvailability(current, currentRoster?.installations) === "reconnecting") {
        setGeneral(previous => {
          const known = previous.get(peerId);
          const value = next(known?.account === account && known.publicKey === peer.public_key ? known : undefined);
          return value ? new Map(previous).set(peerId, value) : previous;
        });
      }
    };
    void Promise.all([
      invoke<MultiplexerState>("mesh_peer_mux_state", { peerId, signal }),
      readPeerOrdinaryTabs(peerId, signal),
    ])
      .then(([snapshot, ordinaryTabs]) => {
        if (!signal.aborted) {
          const sessions = projectPairedGeneral(peerId, snapshot);
          const teamNames = new Map((snapshot.teams ?? []).flatMap(team => {
            const name = team.name?.trim();
            return name && name !== team.id ? [[team.id, name] as const] : [];
          }));
          const workspaces = projectBrowsedWorkspaces(peerId, snapshot, ordinaryTabs);
          const lastActiveTab = resolvePeerLastActiveTab(snapshot, sessions, workspaces, ordinaryTabs);
          const unnamedTeamIds = unnamedTeamIdsForWorkspaces(workspaces);
          const cachedTitles = planTitlesCache.current.get(peerId);
          const validCache = cachedTitles?.publicKey === peer.public_key ? cachedTitles : undefined;
          const initialWorkspaces = validCache && validCache.planTitles.size
            ? workspaces.map(workspace => ({ ...workspace, planTitles: validCache.planTitles }))
            : workspaces;
          publish(() => ({
            account,
            publicKey: peer.public_key,
            sessions,
            teamNames,
            workspaces: initialWorkspaces,
            lastActiveTab,
            availability: "online",
            revision: ++snapshotRevisionRef.current,
          }));
          const needsPlanTitle = needsPeerPlanTitlesFetch(unnamedTeamIds, cachedTitles, peer.public_key);
          if (!needsPlanTitle) return;
          // Return this promise so `reads.current` keeps peer refreshes serialized until `/api/plans` settles.
          return readTeamPlanTitles(peerId, signal).then(planTitles => {
            if (!planTitles) return;
            const lookedUpTeamIds = new Set(validCache?.lookedUpTeamIds ?? []);
            for (const teamId of unnamedTeamIds) lookedUpTeamIds.add(teamId);
            planTitlesCache.current.set(peerId, { publicKey: peer.public_key, planTitles, lookedUpTeamIds });
            publish(previous => previous?.sessions !== sessions ? undefined : ({
              ...previous,
              workspaces: workspaces.map(workspace => ({ ...workspace, planTitles })),
            }));
          });
        }
      })
      .catch(() => publish(previous => ({
        account,
        publicKey: peer.public_key,
        sessions: previous?.sessions,
        teamNames: previous?.teamNames,
        workspaces: previous?.workspaces,
        lastActiveTab: previous?.lastActiveTab,
        availability: "unreachable",
        revision: ++snapshotRevisionRef.current,
      })))
      .finally(() => {
        if (reads.current.get(peerId) !== read) return;
        reads.current.delete(peerId);
        if (read.again) refreshPeerInventory(peerId);
      });
  }, []);

  useEffect(() => {
    const running = reads.current;
    return () => {
      for (const read of running.values()) read.controller.abort();
      running.clear();
    };
  }, []);

  useEffect(() => {
    const peers = peerRoster.current;
    if (!peers) return;
    const paired = new Set(peers.paired.map(peer => peer.instance_id));
    for (const [peerId, read] of reads.current) {
      const peer = peers.paired.find(item => item.instance_id === peerId);
      if (read.account === cacheAccount && peer && pairedAvailability(peer, peers.installations) === "reconnecting") continue;
      reads.current.delete(peerId);
      read.controller.abort();
    }
    setGeneral(previous => {
      const missing = [...previous.keys()].filter(id => !paired.has(id));
      if (!missing.length) return previous;
      const next = new Map(previous);
      for (const id of missing) next.delete(id);
      return next;
    });
    for (const id of paired) refreshPeerInventory(id);
  }, [rosterRevision, refreshPeerInventory, cacheAccount]);

  const target = useWorkspaceBrowseTarget();
  const targetId = target?.id;
  const selected = peers?.paired.find(peer => peer.instance_id === targetId);
  const rosterReady = peers !== null;
  const selectedKey = selected?.public_key;

  useEffect(() => {
    if (!targetId || !rosterReady) return;
    if (selectedKey === undefined) {
      setWorkspaceBrowseTarget(null);
      return;
    }
    refreshPeerInventory(targetId);
  }, [targetId, rosterReady, selectedKey, refreshPeerInventory]);

  if (target && switchSyncRef.current?.target !== target) {
    switchSyncRef.current = { target, baseRevision: general.get(target.id)?.revision ?? 0 };
  } else if (!target) {
    switchSyncRef.current = null;
  }
  const switchBaseRevision = target && switchSyncRef.current?.target === target ? switchSyncRef.current.baseRevision : 0;

  const browse = useMemo<WorkspaceBrowse | null>(() => {
    if (!target) return null;
    const availability = selected ? pairedAvailability(selected, peers?.installations) : "reconnecting";
    const snapshot = general.get(target.id);
    const snapshotValid = Boolean(
      selected && snapshot?.account === cacheAccount && snapshot.publicKey === selected.public_key,
    );
    const synced = peers !== null
      && (availability !== "reconnecting" || (snapshotValid && (snapshot?.revision ?? 0) > switchBaseRevision));
    if (availability === "reconnecting" && snapshotValid && snapshot) {
      return { target, availability: snapshot.availability, workspaces: snapshot.workspaces ?? [], synced };
    }
    return { target, availability, workspaces: [], synced };
  }, [target, selected, peers, general, cacheAccount, switchBaseRevision]);

  const generals = useMemo<PairedGeneral[]>(() => (peers?.paired ?? []).map(peer => {
    const live = general.get(peer.instance_id);
    const saved = cachedPeers?.get(peer.instance_id);
    const known = live?.account === cacheAccount && live.publicKey === peer.public_key ? live : undefined;
    const observed = pairedAvailability(peer, peers?.installations);
    return {
      peerId: peer.instance_id,
      name: accountNames.peers.get(peer.instance_id) ?? cleanMachineName(peer.instance_name, peer.instance_id) ?? "Machine",
      teamNames: known?.teamNames,
      lastActiveTab: known?.lastActiveTab,
      availability: observed === "reconnecting" ? known?.availability ?? observed : observed,
      sessions: known?.sessions ?? (saved?.publicKey === peer.public_key ? saved.sessions : []),
      cached: !known?.sessions && saved !== undefined && saved.publicKey === peer.public_key,
    };
  }), [general, peers, cachedPeers, cacheAccount, accountNames]);
  const peerWorkspaces = useMemo(() => generals.flatMap(peer => {
    const paired = peers?.paired.find(item => item.instance_id === peer.peerId);
    const known = general.get(peer.peerId);
    if (!known || known.account !== cacheAccount || known.publicKey !== paired?.public_key) return [];
    if (browse?.target.id === peer.peerId && browse.availability === "online") {
      const planTitles = known.workspaces?.[0]?.planTitles;
      return browse.workspaces.map(workspace => ({ ...workspace, planTitles }));
    }
    return known.workspaces ?? [];
  }), [generals, peers, general, cacheAccount, browse]);
  return { generals, browse, peerWorkspaces, refreshPeerInventory, invalidatePeerInventory, ready };
}

async function readPeerOrdinaryTabs(peerId: string, signal: AbortSignal): Promise<PeerOrdinaryTabSummary[]> {
  try {
    const tabs = await daemonTransportForPeer(peerId).json<PeerOrdinaryTabSummary[]>("GET", "/api/tabs", undefined, { signal });
    return Array.isArray(tabs) ? tabs : [];
  } catch {
    return [];
  }
}

async function readTeamPlanTitles(peerId: string, signal: AbortSignal) {
  try {
    const { plans } = await daemonTransportForPeer(peerId).json<{ plans: PlanIndexEntry[] }>(
      "GET", "/api/plans", undefined, { signal },
    );
    const latest = new Map<string, PlanIndexEntry>();
    for (const plan of plans) {
      if (plan.team_id && plan.title && (latest.get(plan.team_id)?.updated_at ?? -Infinity) < plan.updated_at) {
        latest.set(plan.team_id, plan);
      }
    }
    return new Map([...latest].map(([teamId, plan]) => [teamId, plan.title]));
  } catch {
    return undefined;
  }
}
