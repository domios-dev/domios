/**
 * Tauri IPC mock for Playwright tests.
 *
 * Injected via page.addInitScript() before the app loads. Provides minimal
 * responses for the IPC commands the frontend calls during initialization so
 * the React app can render in a plain browser without the native Tauri shell.
 */

// Default theme (matches useConfig.ts DEFAULT_THEME)
var DEFAULT_THEME = {
  name: "github-dark",
  background: "#0d1117",
  foreground: "#c9d1d9",
  cursor: "#58a6ff",
  selection: "#264f78",
  black: "#0d1117",
  red: "#ff7b72",
  green: "#7ee787",
  yellow: "#d29922",
  blue: "#58a6ff",
  magenta: "#bc8cff",
  cyan: "#39d353",
  white: "#c9d1d9",
  bright_black: "#484f58",
  bright_red: "#ffa198",
  bright_green: "#56d364",
  bright_yellow: "#e3b341",
  bright_blue: "#79c0ff",
  bright_magenta: "#d2a8ff",
  bright_cyan: "#56d364",
  bright_white: "#f0f6fc",
};

var DEFAULT_CONFIG = {
  theme: DEFAULT_THEME,
  font: {
    family: "JetBrains Mono, Cascadia Code, Fira Code, Consolas, monospace",
    size: 14,
    ligatures: true,
  },
  terminal: {
    cursor_style: "block",
    cursor_blink: true,
    scrollback: 10000,
    copy_on_select: false,
  },
  notifications: {
    desktop: true,
    sound: false,
    toast_duration: 5,
  },
  keybindings: {
    new_tab: "Ctrl+Shift+T",
    close_tab: "Ctrl+Shift+W",
    next_tab: "Ctrl+Tab",
    prev_tab: "Ctrl+Shift+Tab",
    split_right: "Ctrl+Shift+D",
    split_down: "Ctrl+Shift+E",
    close_pane: "Ctrl+Shift+Q",
    focus_pane_left: "Alt+Left",
    focus_pane_right: "Alt+Right",
    focus_pane_up: "Alt+Up",
    focus_pane_down: "Alt+Down",
    new_workspace: "Ctrl+Shift+N",
    copy: "Ctrl+Shift+C",
    paste: "Ctrl+Shift+V",
    search: "Ctrl+Shift+F",
    zoom_in: "Ctrl+=",
    zoom_out: "Ctrl+-",
    zoom_reset: "Ctrl+0",
    open_browser: "Ctrl+Shift+B",
    open_settings: "Ctrl+,",
  },
  socket: { path: null },
  agent: {
    poll_interval_ms: 500,
    idle_timeout_s: 30,
    resume_sessions: true,
    resume_skip_permissions: false,
  },
  api: {
    enabled: true,
    host: "0.0.0.0",
    port: 7685,
    token: null,
    cors: true,
  },
  onboarding_completed: true,
};

// Fake pane and workspace for initial state
var MOCK_PANE_ID = "test-pane-001";
var MOCK_WORKSPACE_ID = "test-ws-001";
var MOCK_TAB_ID = "test-tab-001";

var MOCK_STATE = {
  workspaces: [
    {
      id: MOCK_WORKSPACE_ID,
      name: "default",
      tabs: [
        {
          id: MOCK_TAB_ID,
          title: "Terminal",
          split_tree: { type: "Leaf", pane_id: MOCK_PANE_ID },
        },
      ],
      active_tab_id: MOCK_TAB_ID,
    },
  ],
  panes: {},
  active_workspace_id: MOCK_WORKSPACE_ID,
  focused_pane_id: MOCK_PANE_ID,
};

// We deliberately do NOT include the pane in panes{} to avoid TerminalPane
// trying to create a real xterm.js terminal (which tries to create a PTY).
// The sidebar/tabs will still render from the workspace structure.

// IPC command handlers
var HANDLERS = {
  mux_get_state: function() { return MOCK_STATE; },
  get_config: function() { return DEFAULT_CONFIG; },
  get_config_path: function() { return "/tmp/codeterm-test-config.toml"; },
  save_session: function() { return null; },
  update_config: function() { return null; },
  mux_create_workspace: function() { return null; },
  mux_set_active_workspace: function() { return null; },
  mux_close_workspace: function() { return null; },
  mux_rename_workspace: function() { return null; },
  mux_set_active_tab: function() { return null; },
  mux_close_tab: function() { return null; },
  mux_close_pane: function() { return null; },
  mux_update_pane_cwd: function() { return null; },
  mux_move_tab: function() { return null; },
  mux_link_pty: function() { return null; },
  kill_pty: function() { return null; },
  write_pty_string: function() { return null; },
  spawn_pty: function() { return { pty_id: "mock-pty-002" }; },
  create_pty: function() { return "mock-pty-002"; },
  read_pty: function() { return ""; },
  resize_pty: function() { return null; },
  install_skill: function() { return null; },
  install_claude_hooks: function() { return null; },
  install_project_rules: function() { return null; },
  get_git_info: function() { return { branch: "main", remote_url: null }; },
  check_skills_status: function() {
    return { skills: [], rules_installed: true, hooks_installed: true };
  },
  list_themes: function() { return []; },
  cron_list: function() { return []; },
  cron_delete: function() { return null; },
};

// Callback registry for Tauri event system
var callbackRegistry = {};
var callbackIdCounter = 1;

// Install mocks on window before Tauri JS API loads
window.__TAURI_INTERNALS__ = {
  invoke: function(cmd, args, options) {
    var handler = HANDLERS[cmd];
    if (handler) {
      return Promise.resolve(handler(args));
    }
    console.warn("[tauri-mock] unhandled invoke: " + cmd, args);
    return Promise.resolve(null);
  },
  transformCallback: function(callback, once) {
    var id = callbackIdCounter++;
    callbackRegistry["_" + id] = { callback: callback, once: once };
    window["_" + id] = function(data) {
      callback(data);
      if (once) {
        delete callbackRegistry["_" + id];
        delete window["_" + id];
      }
    };
    return id;
  },
  metadata: {
    currentWindow: { label: "main" },
    currentWebview: { label: "main" },
  },
};

// Mark test mode
window.__CODETERM_TEST_MOCK__ = {
  state: MOCK_STATE,
  config: DEFAULT_CONFIG,
  handlers: HANDLERS,
  emitEvent: function(event, payload) {
    // Emit to all registered listeners for this event
    Object.keys(callbackRegistry).forEach(function(key) {
      var entry = callbackRegistry[key];
      if (entry && entry.callback) {
        try { entry.callback({ event: event, payload: payload, id: 0 }); } catch(e) {}
      }
    });
  },
};
