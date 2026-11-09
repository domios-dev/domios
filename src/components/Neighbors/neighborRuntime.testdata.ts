import type { ChannelStatus, NeighborPermission } from "./neighborRuntimeClient";

export function channelStatus(patch: Partial<ChannelStatus> = {}): ChannelStatus {
  return { enrolled: false, enabled: true, machine_label: "This machine", bindings: {},
    watches: {}, incoming_handling: {}, permissions: {}, concierges: {},
    friend_receivers: {}, friend_receivers_version: 0, notification_preferences: {},
    listeners: {}, listener_errors: {}, ...patch };
}

export function permissionResponse(patch: Partial<NeighborPermission> = {}): NeighborPermission {
  return { allowed: false, scope: "", allow_tab_requests: false, allow_interactive_content: false,
    allow_memory: true, allow_files: true, folder: null, machine_access: "read_only",
    memory_areas: null, memory_write_areas: [], workspaces: null, initial_context_digest: null,
    effective_folder: "/friend", folder_enforcement: {
      writes: "advisory", reads: "not_confined", network: "not_confined", note: "",
    }, ...patch };
}

export function conciergeState(patch: Partial<ChannelStatus["concierges"][string]> = {}): ChannelStatus["concierges"][string] {
  return { concierge_dormant: false, reasoning_effort: null, service_tier: null, initial_context_digest: null,
    gateway_tab: null, paused: false, my_paused: false, concierge_missing: true, pending_count: null,
    pending_count_capped: false, receiving_here: false, receiver_machine_label: null, state: "concierge_missing", ...patch };
}
