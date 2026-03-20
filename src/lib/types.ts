export interface Workspace {
  id: string;
  name: string;
  tabs: Tab[];
  active_tab_id: string | null;
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

export interface Pane {
  id: string;
  pty_id: string | null;
  pane_type: "Terminal" | "Browser";
  title: string;
  cwd: string | null;
  url?: string;
  pinned_title?: boolean;
  agent_type?: "claude" | null;
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

export interface Notification {
  id: string;
  pane_id: string | null;
  workspace_id: string | null;
  title: string;
  body: string | null;
  level: "Info" | "Warning" | "Error";
  timestamp: number;
  read: boolean;
}

export type PaneStatus = "Working" | "Waiting" | "Idle" | "Dead";

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
}

export interface ApiConfig {
  enabled: boolean;
  host: string;
  port: number;
  token: string | null;
  cors: boolean;
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
}
