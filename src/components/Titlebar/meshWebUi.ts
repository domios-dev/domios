/**
 * Pure view derivation for the global Mesh "Web UI" header control. Maps the
 * backend reconciliation snapshot (webAccess::ServeReconcileSnapshot) onto the
 * exact affordances the header renders — never inferring state from peer cards
 * or the canvas. The component stays a thin renderer of what this returns.
 */
import type { ServeReconcileSnapshot } from "../../lib/webAccess";

export type WebUiTone = "ready" | "warn" | "conflict" | "error" | "neutral";

export interface WebUiView {
  /** `data-web-ui-state`: the backend state verbatim, or "loading"/"unknown". */
  state: string;
  /** Compact chip label: Ready · Setup required · Conflict · Unhealthy · … */
  label: string;
  tone: WebUiTone;
  url: string | null;
  canSetup: boolean;
  canRepair: boolean;
  canCopyUrl: boolean;
  canDisable: boolean;
  operatorBootstrap: boolean;
  bootstrapCommand: string | null;
  /** One truthful supplementary line (conflict reason / health error / skew). */
  detail: string | null;
}

function ownsRoute(snap: ServeReconcileSnapshot): boolean {
  if (snap.route?.ownership === "owned") return true;
  return (snap.routes ?? []).some((r) => r.ownership === "owned");
}

function readyDetail(snap: ServeReconcileSnapshot): string | null {
  if (snap.binary.version_skew) {
    const { cli_version, daemon_version } = snap.binary;
    return `Tailscale CLI/daemon version skew (${cli_version ?? "?"} vs ${daemon_version ?? "?"}).`;
  }
  if (snap.health.build_matches === false) {
    const served = snap.health.daemon_build ?? "unknown";
    const expected = snap.health.expected_build ?? "this build";
    return `Serving a different build (${served}) than ${expected}.`;
  }
  return null;
}

/** Derive the header view. `loading` only matters before the first snapshot;
 * once a snapshot exists it is the sole source of truth. */
export function deriveWebUiView(
  snapshot: ServeReconcileSnapshot | null | undefined,
  loading: boolean,
): WebUiView {
  if (!snapshot) {
    return {
      state: loading ? "loading" : "unknown",
      label: loading ? "Checking…" : "Unavailable",
      tone: "neutral",
      url: null,
      canSetup: false,
      canRepair: false,
      canCopyUrl: false,
      canDisable: false,
      operatorBootstrap: false,
      bootstrapCommand: null,
      detail: loading ? null : "Web access status is unavailable.",
    };
  }

  const url = snapshot.url ?? null;
  const owned = ownsRoute(snapshot);
  const base = {
    state: snapshot.state,
    url,
    canCopyUrl: Boolean(url),
    canDisable: owned,
    operatorBootstrap: snapshot.operator_bootstrap_required,
    bootstrapCommand: snapshot.bootstrap_command ?? null,
  };

  switch (snapshot.state) {
    case "ready":
      return {
        ...base,
        label: "Ready",
        tone: "ready",
        canSetup: false,
        canRepair: false,
        detail: readyDetail(snapshot),
      };
    case "operator_bootstrap_required":
      return {
        ...base,
        label: "Setup required",
        tone: "warn",
        canSetup: true,
        canRepair: false,
        detail: snapshot.bootstrap_command
          ? `Operator bootstrap needed: ${snapshot.bootstrap_command}`
          : "Operator bootstrap needed before Serve can bind the HTTPS port.",
      };
    case "setup_required":
      return {
        ...base,
        label: "Setup required",
        tone: "warn",
        canSetup: true,
        canRepair: false,
        detail: snapshot.binary.installed
          ? "Tailscale Serve isn't configured for this daemon yet."
          : "Install Tailscale to publish a clean HTTPS link.",
      };
    case "conflict":
      return {
        ...base,
        label: "Conflict",
        tone: "conflict",
        canSetup: false,
        canRepair: true,
        detail:
          snapshot.conflict ??
          "Another Serve route already occupies the HTTPS port.",
      };
    case "unhealthy":
      return {
        ...base,
        label: "Unhealthy",
        tone: "error",
        canSetup: false,
        canRepair: true,
        detail:
          snapshot.health.error ??
          (snapshot.health.status
            ? `Web UI health check returned ${snapshot.health.status}.`
            : "The published URL is not responding."),
      };
    default:
      return {
        ...base,
        label: "Unavailable",
        tone: "neutral",
        canSetup: false,
        canRepair: true,
        detail: "Unrecognized web access state.",
      };
  }
}
