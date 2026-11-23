import claudeIcon from "../../../../plugins/provider-claude/icon.svg?raw";
import codexIcon from "../../../../plugins/provider-codex/icon.svg?raw";
import opencodeIcon from "../../../../plugins/provider-opencode/icon.svg?raw";
import type { InstalledPlugin, PluginCapabilities, PluginSnapshot } from "../../../../src/components/Extensions/types";
import type { SettingField } from "../../../../src/lib/types";
import bitwardenIcon from "./plugin-icons/bitwarden.svg?raw";
import gitIcon from "./plugin-icons/git.svg?raw";
import larkIcon from "./plugin-icons/lark.svg?raw";
import pebbleIcon from "./plugin-icons/pebble.svg?raw";
import teamsIcon from "./plugin-icons/teams-client.svg?raw";
import transcriberIcon from "./plugin-icons/transcriber.svg?raw";
import { filmRoute } from "./daemon";

const CORE = "codeterm-core";
const PLUGINS = "codeterm-plugins";

type Row = { id: string; name: string; version: string; description: string; icon?: string; color?: string; capabilities?: PluginCapabilities; remote?: string; settings?: SettingField[] };

function plugin(channel: string, { id, name, version, description, icon, color, capabilities, remote, settings }: Row): InstalledPlugin {
  const system = channel === CORE;
  return {
    id, display_name: name, version, description, state: "active", system, removable: !system, verified: true, capabilities: capabilities ?? null,
    icon_svg: icon ?? null, icon_color: color ?? null, settings_schema: settings ?? null,
    source: system ? { kind: "system_channel", channel, path: `plugins/${id}` } : { kind: "channel", channel, path: id },
    update: remote ? { available: true, reason: "newer channel release", channel, remote_version: remote } : { available: false, reason: "current" },
  };
}

// Names, versions, descriptions and icons come from the shipped plugins/ channel and the codeterm-plugins manifests.
const CORE_ROWS: Row[] = [
  { id: "claude", name: "Claude Code", version: "1.14.20", description: "Anthropic's coding agent in a PTY tab, with hooks, models and session resume.", icon: claudeIcon, color: "#d97757", capabilities: { agent_provider: true } },
  { id: "codex", name: "Codex", version: "1.14.49", description: "OpenAI Codex as a PTY agent or a structured chat backend.", icon: codexIcon, color: "var(--ct-fg)", capabilities: { agent_provider: true, chat_backend: true } },
  { id: "opencode", name: "OpenCode", version: "1.14.10", description: "OpenCode agent with configurable model providers.", icon: opencodeIcon, color: "var(--ct-fg)", capabilities: { agent_provider: true } },
  { id: "browser", name: "Browser", version: "1.12.4", description: "An agent-owned live browser: the agent drives a real Chromium, the user watches it inside the agent's tab." },
  { id: "analytics", name: "Analytics", version: "1.10.21", description: "Machine-wide token totals, time series and tab counts, grouped by provider, model, workspace, team or plan.", capabilities: { view: true } },
];

// The Transcriber's settings.schema.json from the codeterm-plugins repo.
const TRANSCRIBER_SETTINGS: SettingField[] = [{
  kind: "section", title: "Transcription", description: "Offline speech-to-text. Dependencies install themselves on first use.",
  fields: [
    { kind: "text", key: "model", label: "Model", placeholder: "small", description: "whisper.cpp model id (ggml-<model>.bin). small = multilingual, good balance. base = faster/lighter; medium / large-v3 = more accurate, slower." },
    { kind: "text", key: "language", label: "Language hint", placeholder: "auto", description: "BCP-47 code (e.g. en, ru). auto detects the language per clip." },
    { kind: "text", key: "endpoint", label: "Remote endpoint", placeholder: "", description: "Optional GPU offload URL (e.g. http://host:7891/transcribe). When set, audio is uploaded there instead of running whisper.cpp locally — no ffmpeg/model download. Leave empty for offline local mode." },
  ],
}];

const PLUGIN_ROWS: Row[] = [
  { id: "pebble", name: "Pebble", version: "0.2.3", description: "Pebble Index ring transcripts, delivered to the tab you pick or to the General Agent.", icon: pebbleIcon, capabilities: { view: true, glance_view: true } },
  { id: "telegram-client", name: "Telegram Client", version: "1.0.0", description: "Sign in to Telegram, find chats by name, read bounded history and send messages for the owner.", color: "#2796D8", capabilities: { view: true, glance_view: true } },
  { id: "teams-client", name: "Teams Client", version: "1.0.2", description: "Sign in to Microsoft Teams, search chats, read history and send messages and files for the owner.", icon: teamsIcon, capabilities: { view: true, glance_view: true } },
  { id: "transcriber", name: "Transcriber", version: "0.4.3", description: "Offline speech-to-text for voice notes; audio never leaves your machine.", icon: transcriberIcon, capabilities: { transcriber: true, glance_view: true }, remote: "0.4.4", settings: TRANSCRIBER_SETTINGS },
  { id: "git", name: "Git", version: "0.1.6", description: "Live Git branch and working-tree status in the pane footer, a glance popover and a full Git view.", icon: gitIcon, capabilities: { status_bubble: true, glance_view: true, view: true } },
  { id: "bitwarden", name: "Bitwarden", version: "1.0.7", description: "Use a Bitwarden vault as Domios's secret store, via the bw CLI.", icon: bitwardenIcon, capabilities: { secret_backend: true, glance_view: true, view: true } },
  { id: "lark", name: "LARK M2", version: "0.1.2", description: "Use the LARK M2 transmitter button to start and stop the voice recorder.", icon: larkIcon, capabilities: { view: true } },
];

export const PLUGIN_SNAPSHOT: PluginSnapshot = {
  plugins: [...CORE_ROWS.map((row) => plugin(CORE, row)), ...PLUGIN_ROWS.map((row) => plugin(PLUGINS, row))],
  channels: [
    { name: CORE, url: "https://github.com/rollacode/codeterm", kind: "git", system: true, removable: false, verified: true, trusted: true },
    { name: PLUGINS, url: "https://github.com/rollacode/codeterm-plugins", kind: "git", system: true, removable: false, verified: true, trusted: true },
  ],
  diff: [],
  marketplace: [],
};

filmRoute("GET", "/api/plugins/snapshot", () => PLUGIN_SNAPSHOT);
filmRoute("GET", "/api/config", () => ({ plugins: { transcriber: { model: "small", language: "auto" } } }));
