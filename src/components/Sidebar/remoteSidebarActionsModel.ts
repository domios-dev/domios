import type { GeneralAvailability } from "../../store/multiplexer/pairedWorkspaceProjection";

export interface RemoteActionDecision {
  visible: boolean;
  disabled: boolean;
  reason: string | null;
}

export interface RemoteTeamMenuDecision {
  canSaveTemplate: boolean;
  renameTeam: RemoteActionDecision;
  closeTeam: RemoteActionDecision;
}

export interface RemoteWorkspaceMenuDecision {
  newSession: RemoteActionDecision;
  newTerminal: RemoteActionDecision;
  renameWorkspace: RemoteActionDecision;
  closeWorkspace: RemoteActionDecision;
}

export function remotePeerStatusLabel(availability: GeneralAvailability | undefined): string {
  switch (availability) {
    case "online":
      return "online";
    case "signed-out":
      return "signed out";
    case "update-required":
      return "update required";
    case "reconnecting":
      return "reconnecting";
    case "unreachable":
      return "unreachable";
    case "offline":
    case undefined:
      return "offline";
  }
}

export function remotePeerActionEligibility(availability: GeneralAvailability | undefined): {
  allowed: boolean;
  reason: string | null;
} {
  if (availability === "online") return { allowed: true, reason: null };
  return { allowed: false, reason: remotePeerStatusLabel(availability) };
}

export function remoteWorkspaceCreateAction(
  peerId: string | null,
  availability?: GeneralAvailability,
): RemoteActionDecision {
  if (peerId === null) return { visible: true, disabled: false, reason: null };
  const eligibility = remotePeerActionEligibility(availability);
  return { visible: true, disabled: !eligibility.allowed, reason: eligibility.reason };
}

export function remoteWorkspaceCloseAction(
  workspaceProtected: boolean,
  availability?: GeneralAvailability,
): RemoteActionDecision {
  if (workspaceProtected) return { visible: false, disabled: true, reason: null };
  const eligibility = remotePeerActionEligibility(availability);
  return { visible: true, disabled: !eligibility.allowed, reason: eligibility.reason };
}

export function remoteWorkspaceMenuActions({
  workspaceProtected,
  cwdMissing = false,
  availability,
}: {
  workspaceProtected: boolean;
  cwdMissing?: boolean;
  availability?: GeneralAvailability;
}): RemoteWorkspaceMenuDecision {
  const eligibility = remotePeerActionEligibility(availability);
  const spawnAction: RemoteActionDecision = cwdMissing
    ? { visible: false, disabled: true, reason: null }
    : { visible: true, disabled: !eligibility.allowed, reason: eligibility.reason };
  const closeWorkspace = remoteWorkspaceCloseAction(workspaceProtected, availability);
  return {
    newSession: spawnAction,
    newTerminal: spawnAction,
    renameWorkspace: workspaceProtected
      ? { visible: false, disabled: true, reason: null }
      : { visible: true, disabled: !eligibility.allowed, reason: eligibility.reason },
    closeWorkspace,
  };
}

export function remoteObservedTabAllowedMenuIds(canOperate: boolean): readonly string[] {
  return canOperate ? ["rename", "restart", "stop", "close"] : [];
}

export function remoteTeamMenuActions({
  hasTeam,
  ownedByPeer,
  availability,
}: {
  hasTeam: boolean;
  ownedByPeer: boolean;
  availability?: GeneralAvailability;
}): RemoteTeamMenuDecision {
  if (!hasTeam || !ownedByPeer) {
    return {
      canSaveTemplate: false,
      renameTeam: { visible: false, disabled: true, reason: null },
      closeTeam: { visible: false, disabled: true, reason: null },
    };
  }
  const eligibility = remotePeerActionEligibility(availability);
  return {
    canSaveTemplate: false,
    renameTeam: { visible: true, disabled: !eligibility.allowed, reason: eligibility.reason },
    closeTeam: { visible: true, disabled: !eligibility.allowed, reason: eligibility.reason },
  };
}

export function remoteTabCloseAction({
  isProtected,
  availability,
  remoteUnavailableReason,
}: {
  isProtected: boolean;
  availability?: GeneralAvailability;
  remoteUnavailableReason?: string | null;
}): RemoteActionDecision {
  if (isProtected) return { visible: false, disabled: true, reason: null };
  if (remoteUnavailableReason) {
    return { visible: true, disabled: true, reason: remoteUnavailableReason };
  }
  if (availability !== undefined && availability !== "online") {
    return { visible: true, disabled: true, reason: remotePeerStatusLabel(availability) };
  }
  return { visible: true, disabled: false, reason: null };
}

export function workspaceNameFromPath(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  return trimmed.split(/[\\/]/).pop() || "Workspace";
}
