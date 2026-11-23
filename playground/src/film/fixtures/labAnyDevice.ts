import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { generateWebQr, type WebAccessActions } from "../../../../src/components/MeshPanel/WebAccessPanel";
import type { NetworkInfo } from "../../../../src/components/MeshPanel/webUiTarget";
import { rememberLocalInstanceName } from "../../../../src/lib/localInstanceName";
import type { AppConfig } from "../../../../src/lib/types";
import type { ServeStatus } from "../../../../src/lib/webAccess";
import { API } from "../../gallery/fixtures";
import { ago } from "./clock";
import { filmRoute } from "./daemon";

export const TAILNET_URL = "https://laptop.tail4f2a9c.ts.net/";
const API_PORT = 7685;

export const WEB_CONFIG = { api: { ...API, port: API_PORT, token: "film-web-token" } } as unknown as AppConfig;
export const WEB_NETWORK: NetworkInfo = { local_ip: "192.168.1.24", tailscale_ip: "100.110.9.24", tailscale_https_url: TAILNET_URL, api_port: API_PORT };
export const WEB_SERVE: ServeStatus = {
  installed: true,
  active: true,
  https_url: TAILNET_URL,
  self_dns: "laptop.tail4f2a9c.ts.net",
  platform: "windows",
  snapshot: {
    state: "ready",
    url: TAILNET_URL,
    api_port: API_PORT,
    desired_https_port: 443,
    chosen_https_port: 443,
    operator_bootstrap_required: false,
    binary: { installed: true, version_skew: false },
    route: { host: "laptop.tail4f2a9c.ts.net", https_port: 443, proxy: `http://127.0.0.1:${API_PORT}`, ownership: "owned" },
    routes: [{ host: "laptop.tail4f2a9c.ts.net", https_port: 443, proxy: `http://127.0.0.1:${API_PORT}`, ownership: "owned" }],
    health: { ok: true, status: 200, build_matches: true },
  },
};

const noop = () => undefined;
export const WEB_ACTIONS: WebAccessActions = { open: noop, copy: async () => undefined, qr: generateWebQr };

type Peer = { instance_id: string; instance_name: string; ip: string; minutes: number };
const PEERS: Peer[] = [
  { instance_id: "gpu-rig", instance_name: "gpu-rig", ip: "100.77.150.3", minutes: 2 },
  { instance_id: "mac-mini", instance_name: "mac-mini", ip: "100.84.12.7", minutes: 9 },
  { instance_id: "homelab", instance_name: "homelab", ip: "100.101.44.18", minutes: 40 },
];

filmRoute("GET", "/api/mesh/peers", () => ({
  local_instance_name: "laptop",
  paired: PEERS.map((p) => ({ instance_id: p.instance_id, instance_name: p.instance_name, addresses: [{ ip: p.ip, is_tailscale: true }], api_port: API_PORT, online: true, relay_protocol_version: 1, last_active_ms: ago(p.minutes) })),
}));
rememberLocalInstanceName("laptop");

export const PHONE_MESSAGE: NormalizedChatMessage = { id: "lab-phone-sent", type: "user", content: "Ship the 4K render of shot 14 tonight." };

export const RENDER_FARM_ROWS: NormalizedChatMessage[] = [
  { id: "lab-rig-ask", type: "user", content: "How far along is shot 14?" },
  { id: "lab-rig-reply", type: "assistant", content: "Shot 14 is rendering at 4K on gpu-rig: **frame 212 of 480**, about 18 minutes left. I'll post the file when it lands." },
];
