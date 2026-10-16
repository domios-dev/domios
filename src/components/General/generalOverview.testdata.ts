import type { Tab, Team, Workspace } from "../../lib/types";
import type { Service } from "../MeshPanel/types";

export const owner: Tab = { id: "owner", title: "Owner", tab_type: "Terminal", agent_type: "fixture", pty_id: "owner-pty", cwd: null };
export const shell: Tab = { id: "shell", title: "QA", tab_type: "Terminal", pty_id: "shell-pty", cwd: null, parent_tab_id: owner.id, team_id: "team", purpose: "qa", shell_command_state: { activity: "idle", last_exit_code: 0 } };
export const tabs = { owner, shell };
export const workspaces: Workspace[] = [{ id: "project", name: "Project", tabs: [owner.id, shell.id], active_tab_id: owner.id, cwd: null }];
export const teams: Team[] = [{ id: "team", name: "Team", lifecycle: "active", created_at: 0, workspace_id: "project" }];

export const daemon: Service = {
  id: "daemon", name: "API daemon", kind: "daemon", health_mode: "http", health_interval: 60,
  health: { status: "healthy" }, push_enabled: false, registered_at: 0, updated_at: 0, enabled: true,
  machine_id: "remote", machine_name: "Remote machine", source_peer_id: "remote",
  provenance: { freshness: "offline", offline: true },
  capabilities: { delete: { allowed: false, unavailable_reason: "offline" } },
};
export const schedule: Service = { ...daemon, id: "cron_backup", name: "Remote backup", kind: "cron", schedule: "0 3 * * *", capabilities: { get: { allowed: false, unavailable_reason: "offline" } } };
