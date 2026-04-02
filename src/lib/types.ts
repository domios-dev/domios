/** Title priority: higher levels cannot be overwritten by lower ones. */
export enum TitleLevel {
  Default = "Default",
  Auto = "Auto",
  Agent = "Agent",
  User = "User",
}

export interface Workspace {
  id: string;
  name: string;
  tabs: Tab[];
  active_tab_id: string | null;
  cwd: string | null;
  protected?: boolean;
}

export interface Tab {
  id: string;
  title: string;
  split_tree: SplitNode;
}

export type SplitNode =
  | { type: "Leaf"; pane_id: string }
  | { type: "Split"; orientation: Orientation; ratio: number; children: SplitNode[] };

export type Orientation = "Horizontal" | "Vertical";

export type AgentRole = "Orchestrator" | "Lead" | "Worker" | "Conductor" | "SubWorker";

export interface PromptOption {
  label: string;
  value: string;
}

export interface PendingPrompt {
  question: string;
  options: PromptOption[];
}

export interface Pane {
  id: string;
  pty_id: string | null;
  pane_type: "Terminal" | "Browser";
  title: string;
  cwd: string | null;
  url?: string;
  title_level?: TitleLevel;
  agent_type?: string | null;
  agent_session_id?: string | null;
  role?: AgentRole | null;
  parent_pane_id?: string | null;
  can_spawn?: boolean;
  depth?: number;
  chat_view?: boolean;
  pending_prompt?: PendingPrompt | null;
  protected?: boolean;
  connected_services?: string[];
}

export interface MultiplexerState {
  workspaces: Workspace[];
  panes: Record<string, Pane>;
  active_workspace_id: string | null;
  focused_pane_id: string | null;
}

export interface WorkspaceInfo {
  id: string;
  name: string;
  tab_count: number;
  is_active: boolean;
  cwd: string | null;
  protected?: boolean;
}

export interface SplitResult {
  new_pane_id: string;
  new_pty_id: string;
}

export interface TabInfo {
  id: string;
  title: string;
  pane_id: string;
}

export interface PtyInfo {
  id: string;
  status: "running" | "dead";
}

export interface NotificationAction {
  label: string;
  /** "invoke" calls a Tauri command, "url" opens in browser, "dismiss" just closes */
  type: "invoke" | "url" | "dismiss";
  /** Tauri command name or URL */
  value?: string;
  /** Args for Tauri invoke */
  args?: Record<string, unknown>;
}

export interface Notification {
  id: string;
  pane_id: string | null;
  workspace_id: string | null;
  title: string;
  body: string | null;
  level: "Info" | "Warning" | "Error";
  timestamp: number;
  read: boolean;
  /** Action buttons — if present, toast shows buttons instead of auto-dismiss */
  actions?: NotificationAction[];
  /** Persist toast — don't auto-dismiss, require explicit action */
  sticky?: boolean;
}

export type PaneStatus = "Working" | "Waiting" | "Idle" | "Dead";

// Cron types

export type CronRunStatus = "Running" | "Success" | "Failed";

export interface CronRunInfo {
  run_id: string;
  started_at: number;
  finished_at: number | null;
  status: CronRunStatus;
  pane_id: string | null;
  tab_id: string | null;
}

export interface CronJob {
  id: string;
  name: string;
  description: string | null;
  workspace_id: string;
  schedule: string;
  enabled: boolean;
  agent: string;
  task: string;
  cwd: string | null;
  role: string | null;
  max_history: number;
  created_at: number;
  last_run: CronRunInfo | null;
  next_run: number | null;
}

export interface GitInfo {
  branch: string | null;
  is_dirty: boolean;
  repo_root: string;
}

// Configuration types (matches Rust AppConfig schema)

export interface ThemeConfig {
  name: string;
  background: string;
  foreground: string;
  cursor: string;
  selection: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  bright_black: string;
  bright_red: string;
  bright_green: string;
  bright_yellow: string;
  bright_blue: string;
  bright_magenta: string;
  bright_cyan: string;
  bright_white: string;
}

export interface FontConfig {
  family: string;
  size: number;
  ligatures: boolean;
}

export interface TerminalConfigSettings {
  cursor_style: string;
  cursor_blink: boolean;
  scrollback: number;
  copy_on_select: boolean;
}

export interface NotificationConfig {
  desktop: boolean;
  sound: boolean;
  toast_duration: number;
}

export interface KeybindingsConfig {
  new_tab: string;
  close_tab: string;
  next_tab: string;
  prev_tab: string;
  split_right: string;
  split_down: string;
  close_pane: string;
  focus_pane_left: string;
  focus_pane_right: string;
  focus_pane_up: string;
  focus_pane_down: string;
  new_workspace: string;
  copy: string;
  paste: string;
  search: string;
  zoom_in: string;
  zoom_out: string;
  zoom_reset: string;
  open_browser: string;
  open_settings: string;
}

export interface SocketConfigSettings {
  path: string | null;
}

export interface AgentConfig {
  poll_interval_ms: number;
  idle_timeout_s: number;
  resume_sessions: boolean;
  resume_skip_permissions: boolean;
}

export interface ApiConfig {
  enabled: boolean;
  host: string;
  port: number;
  token: string | null;
  cors: boolean;
}

export interface GeneralAgentConfig {
  enabled: boolean;
  agent_provider: string;
  auto_restart: boolean;
  max_restart_attempts: number;
  restart_delay_s: number;
}

export interface IntegrationsConfig {
  telegram_enabled: boolean;
  telegram_allowed_users: string[];
  discord_enabled: boolean;
  discord_allowed_users: string[];
  webhook_enabled: boolean;
  webhook_port: number;
}

export interface MeshConfig {
  enabled: boolean;
  instance_name: string;
  auto_discover: boolean;
  stealth: boolean;
}

export interface MeshStatus {
  enabled: boolean;
  instance_id: string;
  instance_name: string;
  is_crown: boolean;
  crown_holder: string | null;
  discovery_active: boolean;
  tailscale_ip: string | null;
  local_ip: string | null;
  api_port: number;
  paired_count: number;
  discovered_count: number;
}

export interface PeerAddress {
  ip: string;
  is_tailscale: boolean;
}

export interface PairedPeer {
  instance_id: string;
  instance_name: string;
  addresses: PeerAddress[];
  api_port: number;
  paired_at: string;
  last_seen: string | null;
  online: boolean;
  is_primary: boolean;
  access_level: string;
  trust_level: string;
  version: string | null;
}

export interface DiscoveredPeer {
  instance_id: string;
  instance_name: string;
  addresses: PeerAddress[];
  api_port: number;
  discovery_method: "mdns" | "tailscale" | "udp" | "manual";
  last_seen: number;
  dev: boolean;
}

export interface MeshPairingRequest {
  request_id: string;
  from_instance_id: string;
  from_instance_name: string;
  from_address: string;
  from_port: number;
  timestamp: number;
  status: "pending" | "accepted" | "rejected" | "expired";
}

export interface MeshPeersResponse {
  paired: PairedPeer[];
  discovered: DiscoveredPeer[];
  incoming_requests: MeshPairingRequest[];
}

export interface AppConfig {
  theme: ThemeConfig;
  font: FontConfig;
  terminal: TerminalConfigSettings;
  notifications: NotificationConfig;
  keybindings: KeybindingsConfig;
  socket: SocketConfigSettings;
  agent: AgentConfig;
  api: ApiConfig;
  general_agent: GeneralAgentConfig;
  integrations: IntegrationsConfig;
  mesh: MeshConfig;
  onboarding_completed: boolean;
}
