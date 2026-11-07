import type { ResizeGeometry } from "./resizeModel";

/** Why a view wants the PTY's size authority. */
export type AuthorityTrigger =
  | "interaction"
  | "activation"
  | "visibility"
  | "window-focus"
  | "reconnect"
  | "background"
  | "passive-render";

export type ClaimMode = "forced" | "unforced" | "none";

/**
 * Only an explicit interaction with this view takes the PTY from another client.
 * Activation adopts an unowned PTY and otherwise follows the owner.
 */
export function claimModeFor(trigger: AuthorityTrigger): ClaimMode {
  switch (trigger) {
    case "interaction":
      return "forced";
    case "activation":
    case "visibility":
    case "window-focus":
      return "unforced";
    default:
      return "none";
  }
}

export interface ViewerAuthority {
  role: "owner" | "follower" | "unclaimed";
  followed: ResizeGeometry | null;
}

export interface ViewerIdentity {
  clientId: string;
  ptyId: string;
}

export interface ClaimAnswer {
  ownerClientId: string;
  ptyId: string;
  ownerSize: ResizeGeometry;
  currentSize: ResizeGeometry | null;
}

export interface OwnerNotice {
  client_id: string;
  pty_id: string;
  cols: number;
  rows: number;
}

export function unclaimedAuthority(): ViewerAuthority {
  return { role: "unclaimed", followed: null };
}

function validGeometry(geometry: ResizeGeometry | null | undefined): geometry is ResizeGeometry {
  return !!geometry && geometry.cols > 0 && geometry.rows > 0;
}

export function authorityAfterClaim(answer: ClaimAnswer, self: ViewerIdentity): ViewerAuthority {
  if (answer.ptyId !== self.ptyId) return unclaimedAuthority();
  if (answer.ownerClientId === self.clientId) return { role: "owner", followed: null };
  const followed = validGeometry(answer.currentSize) ? answer.currentSize : answer.ownerSize;
  return { role: "follower", followed: validGeometry(followed) ? followed : null };
}

export function authorityAfterOwnerChanged(
  state: ViewerAuthority,
  owner: OwnerNotice,
  self: ViewerIdentity,
): ViewerAuthority {
  if (owner.pty_id !== self.ptyId || owner.client_id === self.clientId) return state;
  const followed = { cols: owner.cols, rows: owner.rows };
  if (!validGeometry(followed)) return state;
  return { role: "follower", followed };
}

/** A follower renders the owner's geometry; everyone else renders its own fit. */
export function renderGeometry(state: ViewerAuthority, fitted: ResizeGeometry): ResizeGeometry {
  return state.role === "follower" && state.followed ? state.followed : fitted;
}
