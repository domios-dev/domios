import type { Tab } from "../../lib/types";

/** Top-level tab fields a sidebar row shows or hands to its menu; a change to any of them re-renders the row. */
const ROW_TAB_FIELDS = [
  "id",
  "title",
  "title_level",
  "purpose",
  "description",
  "agent_type",
  "agent_session_id",
  "agent_session_model",
  "agent_provider_resolved_model",
  "agent_requested_model",
  "agent_booting",
  "session_binding_nonce",
  "pty_id",
  "protected",
  "locked",
  "frozen",
  "slept_at_ms",
  "boot_stage",
  "boot_failure_reason",
  "role",
  "role_profile",
  "roleless_tracked",
  "has_checklist",
  "remote_peer_id",
  "remote_peer_name",
  "chat_view",
  "permission_auto_confirm",
  "pending_prompt",
  "parent_tab_id",
  "depth",
] as const satisfies readonly (keyof Tab)[];

function chatBackendIdentity(tab: Tab): string | undefined {
  return tab.backing?.kind === "chat_backend" ? `${tab.backing.plugin_id}\u0000${tab.backing.session_id}` : tab.backing?.kind;
}

/**
 * Whether two snapshots of a tab render the same sidebar row. Fields the row never reads, such as
 * elapsed timers and activity timestamps, change constantly and must not re-render every row.
 */
export function sameRowTab(previous: Tab, next: Tab): boolean {
  if (previous === next) return true;
  return ROW_TAB_FIELDS.every((field) => previous[field] === next[field])
    && chatBackendIdentity(previous) === chatBackendIdentity(next)
    && previous.shell_command_state?.activity === next.shell_command_state?.activity
    && previous.shell_command_state?.last_exit_code === next.shell_command_state?.last_exit_code
    && previous.ownership?.controller_title === next.ownership?.controller_title
    && previous.ownership?.team?.id === next.ownership?.team?.id
    && previous.agent_runtime_config?.confirmedServiceTier === next.agent_runtime_config?.confirmedServiceTier
    && previous.agent_state?.state === next.agent_state?.state
    && previous.agent_state?.status_line === next.agent_state?.status_line
    && previous.agent_state?.phase === next.agent_state?.phase
    && previous.agent_state?.subtype === next.agent_state?.subtype
    && JSON.stringify(previous.provider_signals?.background?.running) === JSON.stringify(next.provider_signals?.background?.running)
    && JSON.stringify(previous.remote_channels) === JSON.stringify(next.remote_channels)
    && JSON.stringify(previous.connected_services) === JSON.stringify(next.connected_services);
}
