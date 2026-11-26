import { useCallback, useEffect, useRef, useState } from "react";
import type { WorkspaceClosePreview } from "../../../gen/WorkspaceClosePreview";
import { daemonTransportForPeer, onDaemonTransport } from "../../lib/daemonBootstrap";
import type { Notice } from "../../lib/notices";
import type { TeamCloseInventory } from "../../lib/teamActions";
import { createOrchestrationTransport } from "../../lib/transport/orchestration";
import type { TransportClient } from "../../lib/transportClient";
import type { MultiplexerState } from "../../lib/types";
import { webRequestContext } from "../../lib/webApi";
import type { GeneralAvailability } from "../../store/multiplexer/pairedWorkspaceProjection";
import type { OrchestrationGroupMember } from "./orchestrationGroupClose";
import { remotePeerActionEligibility } from "./remoteSidebarActionsModel";
import { startBackgroundClose, teamCloseOutcome } from "./teamCloseOutcome";
import type { TeamCloseResource } from "./teamCloseResources";

export interface RemoteTeamCloseTarget {
  peerId: string;
  workspaceId: string;
  teamId: string;
  teamName: string;
  machineName: string;
  availability: GeneralAvailability;
  members: OrchestrationGroupMember[];
}

export type RemoteTeamClosePreview = RemoteTeamCloseTarget & (
  | { phase: "loading" }
  | { phase: "error"; error: string }
  | ({ phase: "ready" } & TeamCloseResource)
);

export interface RemoteTeamClosePort {
  readInventory(peerId: string, workspaceId: string, teamId: string, signal: AbortSignal): Promise<TeamCloseResource>;
  closeTeam(peerId: string, teamId: string): Promise<TeamCloseInventory>;
  teamExists(peerId: string, teamId: string): Promise<boolean>;
}

export function remoteTeamKey(peerId: string, teamId: string): string {
  return JSON.stringify([peerId, teamId]);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function peerRefusalReason(raw: string): string {
  return raw.replace(/^.*? → HTTP \d{3}: /, "").trim() || raw.trim();
}

function teamCloseRefusalDetail(message: string, cause: string, raw: string): string {
  return JSON.stringify({ code: "team_close_refused", message, cause, raw });
}

function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === "AbortError";
}

export const remoteTeamClosePort: RemoteTeamClosePort = {
  async readInventory(peerId, workspaceId, teamId, signal) {
    const transport = daemonTransportForPeer(peerId);
    const tokenBefore = transport.currentToken?.() ?? null;
    const contextBefore = webRequestContext();
    const preview = await transport.json<WorkspaceClosePreview>(
      "GET",
      `/api/workspaces/${encodeURIComponent(workspaceId)}/close-preview`,
      undefined,
      { signal },
    );
    if (webRequestContext() !== contextBefore || (transport.currentToken?.() ?? null) !== tokenBefore) {
      throw new DOMException("Credentials changed", "AbortError");
    }
    if (preview.installation_id !== peerId || preview.workspace_id !== workspaceId) {
      throw new Error("The close preview does not match the selected machine and workspace.");
    }
    const team = preview.teams.find(candidate => candidate.id === teamId);
    if (!team) throw new Error("The selected Team was not found on this machine.");
    if (team.remote) throw new Error("This Team is owned by another machine and cannot be closed here.");
    return {
      id: team.id,
      name: team.name,
      members: team.members.map(member => ({
        tabId: member.tab_id,
        title: member.title,
        role: member.role,
        roleProfile: member.role_profile,
      })),
      crons: team.crons,
      blueprints: team.blueprints,
      lanes: [
        ...team.tasks.map(task => `${task.plan_title}: ${task.task_name}${task.orphan ? " (orphan task)" : ""}${task.remote_attempt ? " (remote attempt)" : ""}`),
        ...team.open_attempts.map(attempt => `Open attempt ${attempt.attempt_id}${attempt.executor_tab_id ? ` · ${attempt.executor_tab_id}` : ""}${attempt.remote ? " (remote)" : ""}`),
        ...team.lanes.map(lane => [lane.path, lane.branch, lane.state, lane.detail].filter(Boolean).join(" · ")),
      ],
    };
  },
  async closeTeam(peerId, teamId) {
    const transport = createOrchestrationTransport(daemonTransportForPeer(peerId));
    const result = await transport.mux_close_team({ teamId, forceBusy: true });
    if (typeof result !== "object" || result === null || !("inventory" in result)) {
      throw new Error("Team close response did not include its lane inventory.");
    }
    return result.inventory;
  },
  async teamExists(peerId, teamId) {
    const state = await daemonTransportForPeer(peerId).json<MultiplexerState>("GET", "/api/mux/state");
    return Boolean(state?.teams?.some(team => team.id === teamId));
  },
};

export function useRemoteTeamClose(
  availabilityForPeer: (peerId: string) => GeneralAvailability | undefined,
  onClosed: (peerId: string, teamId: string) => void,
  notify: (notice: Notice) => void,
  port: RemoteTeamClosePort = remoteTeamClosePort,
) {
  const [pending, setPending] = useState<RemoteTeamClosePreview | null>(null);
  const [closingTeamKeys, setClosingTeamKeys] = useState<ReadonlySet<string>>(() => new Set());
  const pendingRef = useRef<RemoteTeamClosePreview | null>(null);
  const inFlight = useRef(new Set<string>());
  const generation = useRef(0);
  const previewRead = useRef<AbortController | null>(null);

  const cancel = useCallback(() => {
    generation.current++;
    previewRead.current?.abort();
    previewRead.current = null;
    pendingRef.current = null;
    setPending(null);
  }, []);

  useEffect(() => () => { previewRead.current?.abort(); }, []);

  useEffect(() => {
    if (typeof onDaemonTransport !== "function") return;
    let initialized = false;
    let previous: TransportClient | null = null;
    return onDaemonTransport((client) => {
      if (!initialized) {
        initialized = true;
        previous = client;
        return;
      }
      if (client === previous) return;
      previous = client;
      cancel();
    });
  }, [cancel]);

  const request = useCallback(async (
    target: RemoteTeamCloseTarget,
    isStillSelected: () => boolean,
  ): Promise<void> => {
    const eligibility = remotePeerActionEligibility(availabilityForPeer(target.peerId));
    if (target.availability !== "online" || !eligibility.allowed) {
      const reason = target.availability !== "online" ? remotePeerActionEligibility(target.availability).reason : eligibility.reason;
      const summary = `${target.teamName} on ${target.machineName} cannot be closed while the machine is ${reason ?? "offline"}.`;
      notify({
        kind: "toast",
        id: `remote-team-close-unavailable:${target.peerId}:${target.teamId}`,
        tabId: null,
        workspaceId: target.workspaceId,
        title: "Team close unavailable",
        detail: teamCloseRefusalDetail(summary, `${target.teamName} on ${target.machineName} stays open.`, summary),
        severity: "Error",
        persistent: true,
        retainInNotifications: true,
      });
      return;
    }
    if (!isStillSelected()) return;
    const requestGeneration = ++generation.current;
    const contextBefore = webRequestContext();
    previewRead.current?.abort();
    const controller = new AbortController();
    previewRead.current = controller;
    const loadingState: RemoteTeamClosePreview = { ...target, phase: "loading" };
    pendingRef.current = loadingState;
    setPending(loadingState);
    const isStale = () => controller.signal.aborted
      || requestGeneration !== generation.current
      || webRequestContext() !== contextBefore
      || !isStillSelected()
      || availabilityForPeer(target.peerId) !== "online";
    try {
      const inventory = await port.readInventory(target.peerId, target.workspaceId, target.teamId, controller.signal);
      if (isStale()) return;
      const ready: RemoteTeamClosePreview = { ...target, ...inventory, phase: "ready" };
      pendingRef.current = ready;
      setPending(ready);
    } catch (error) {
      if (isStale() || isAbortError(error)) return;
      const failed: RemoteTeamClosePreview = { ...target, phase: "error", error: errorMessage(error) };
      pendingRef.current = failed;
      setPending(failed);
    } finally {
      if (previewRead.current === controller) previewRead.current = null;
    }
  }, [availabilityForPeer, notify, port]);

  const confirm = useCallback(async (): Promise<void> => {
    const target = pendingRef.current;
    if (!target || target.phase !== "ready") return;
    const key = remoteTeamKey(target.peerId, target.teamId);
    if (inFlight.current.has(key)) return;
    const eligibility = remotePeerActionEligibility(availabilityForPeer(target.peerId));
    if (!eligibility.allowed) {
      cancel();
      const summary = `${target.teamName} on ${target.machineName} is ${eligibility.reason ?? "offline"}.`;
      notify({
        kind: "toast",
        id: `remote-team-close:${target.peerId}:${target.teamId}`,
        tabId: null,
        workspaceId: target.workspaceId,
        title: "Team close not sent",
        detail: teamCloseRefusalDetail(summary, `${target.teamName} on ${target.machineName} stays open.`, summary),
        severity: "Error",
        persistent: true,
        retainInNotifications: true,
      });
      return;
    }
    inFlight.current.add(key);
    setClosingTeamKeys(ids => new Set([...ids, key]));
    let inventory: TeamCloseInventory = { removed: [], kept: [], orphan_branches: [] };
    const survivors: string[] = [];
    let verificationUnavailable = false;
    let closeFailure: unknown;
    let closeFailed = false;
    try {
      inventory = await startBackgroundClose(cancel, () => port.closeTeam(target.peerId, target.teamId));
      onClosed(target.peerId, target.teamId);
    } catch (error) {
      closeFailure = error;
      closeFailed = true;
      try {
        if (await port.teamExists(target.peerId, target.teamId)) {
          survivors.push(`Team ${target.teamName} is still open on ${target.machineName}`);
        } else {
          onClosed(target.peerId, target.teamId);
        }
      } catch {
        verificationUnavailable = true;
      }
    } finally {
      inFlight.current.delete(key);
      setClosingTeamKeys(ids => new Set([...ids].filter(id => id !== key)));
    }
    const outcome = teamCloseOutcome(inventory, survivors);
    if (outcome.kind === "closed" && !verificationUnavailable && !closeFailed) return;
    const targetLabel = `${target.teamName} on ${target.machineName}`;
    const title = outcome.kind === "partial"
      ? "Team remains open"
      : verificationUnavailable
        ? "Team close status unavailable"
        : closeFailed
          ? "Team close refused"
          : "Lanes preserved";
    const failureText = closeFailure != null ? errorMessage(closeFailure) : "";
    const rawDetail = [
      targetLabel,
      verificationUnavailable && outcome.kind !== "partial"
        ? "The close could not be verified on the remote machine."
        : (outcome.kind === "partial" ? outcome.survivors : outcome.preserved).join("\n"),
      failureText,
    ].filter(Boolean).join("\n");
    const isError = closeFailed || verificationUnavailable || outcome.kind === "partial";
    const detail = isError
      ? teamCloseRefusalDetail(
        failureText
          ? peerRefusalReason(failureText)
          : verificationUnavailable
            ? "The close could not be verified on the remote machine."
            : `${targetLabel} stays open.`,
        outcome.kind === "partial"
          ? `${targetLabel} stays open.`
          : verificationUnavailable
            ? `Refresh ${target.machineName} before retrying.`
            : `${targetLabel} reported an error during close.`,
        rawDetail,
      )
      : rawDetail;
    notify({
      kind: "toast",
      id: `remote-team-close:${target.peerId}:${target.teamId}`,
      tabId: null,
      workspaceId: target.workspaceId,
      title,
      detail,
      severity: isError ? "Error" : "Info",
      ...(isError ? { persistent: true, retainInNotifications: true } : {}),
    });
  }, [availabilityForPeer, cancel, notify, onClosed, port]);

  return { pending, closingTeamKeys, request, cancel, confirm };
}
