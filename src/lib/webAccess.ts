/**
 * Web Access URL selection — the single source of truth for "what URL does a
 * browser/phone use to reach this CodeTerm". Shared by Settings → Network and
 * the Mesh "Web UI" QR modal so they can never disagree.
 *
 * Priority (best first):
 *   1. Tailscale Serve HTTPS FQDN  (https://host[:port]) — clean, phone-friendly,
 *      no mixed-content. Backend only reports this when Serve actually proxies
 *      to THIS daemon's port (see mesh::discovery::tailscale_serve_https_url),
 *      so it is never a dead 502 link.
 *   2. Tailscale IP + port  (http://100.x.y.z:PORT)
 *   3. LAN IP + port        (http://192.168.x.y:PORT)
 *   4. Loopback             (http://127.0.0.1:PORT)
 *
 * URLs are bare daemon roots. Append the API token as a fragment with
 * webLinkWithToken() when building a one-tap link or QR.
 */

export interface WebAccessNetworkInfo {
  local_ip?: string | null;
  tailscale_ip?: string | null;
  tailscale_https_url?: string | null;
  api_port?: number;
}

export interface WebAccessUrls {
  /** Best URL per the priority chain (always defined — loopback is the floor). */
  primary: string;
  /** Deduped list, primary first, for display. */
  urls: string[];
  /** True when no Serve HTTPS URL is available — the curriculum hint applies. */
  usingFallback: boolean;
}

export function buildWebAccessUrls(
  net: WebAccessNetworkInfo | null,
  fallbackPort: number,
): WebAccessUrls {
  const port = net?.api_port ?? fallbackPort;
  const https = net?.tailscale_https_url ? `${net.tailscale_https_url}/` : null;
  const list = [
    https,
    net?.tailscale_ip ? `http://${net.tailscale_ip}:${port}/` : null,
    net?.local_ip ? `http://${net.local_ip}:${port}/` : null,
    `http://127.0.0.1:${port}/`,
  ].filter((v): v is string => Boolean(v));
  const urls = [...new Set(list)];
  return { primary: urls[0], urls, usingFallback: !https };
}

/** Append the API token as a URL fragment (never a query — fragments aren't sent
 * to the server and survive the FQDN root form). */
export function webLinkWithToken(url: string, token?: string | null): string {
  return token ? `${url}#token=${encodeURIComponent(token)}` : url;
}

export interface WebRelayTarget {
  id: string;
  name: string;
}

export function webLinkWithTokenAndRelay(
  url: string,
  token: string | null | undefined,
  target: WebRelayTarget | null,
): string {
  const fragment = new URLSearchParams();
  if (token) fragment.set("token", token);
  fragment.set("relay", target?.id ?? "self");
  if (target) fragment.set("relay_name", target.name);
  return `${url.replace(/#.*$/, "")}#${fragment.toString()}`;
}

/** Curriculum hint shown when falling back to an IP+port URL: the one command
 * that turns a raw IP link into a clean, phone-friendly HTTPS URL. */
export function serveHint(port: number): string {
  return `Tip: run 'tailscale serve --bg --https=443 http://127.0.0.1:${port}' for a clean HTTPS URL — phones prefer it.`;
}

// ── One-click "Enable Web Access" (Tailscale Serve auto-setup) ──────────────

/** Preferred public HTTPS port — phone-friendly, no port suffix in the URL.
 * Mirrors `serve_setup::DEFAULT_HTTPS_PORT` on the backend. */
const DEFAULT_HTTPS_PORT = 443;

/** Backend `serve_setup::ServeStatus` (serde snake_case). */
export interface ServeStatus {
  installed: boolean;
  active: boolean;
  https_url?: string | null;
  self_dns?: string | null;
  platform?: string | null;
  snapshot?: ServeReconcileSnapshot | null;
}

export type ServeReconcileState =
  | "ready"
  | "setup_required"
  | "operator_bootstrap_required"
  | "conflict"
  | "unhealthy";

export interface ServeReconcileSnapshot {
  state: ServeReconcileState;
  url?: string | null;
  api_port: number;
  desired_https_port: number;
  chosen_https_port?: number | null;
  operator_bootstrap_required: boolean;
  bootstrap_command?: string | null;
  binary: {
    installed: boolean;
    path?: string | null;
    cli_version?: string | null;
    daemon_version?: string | null;
    version_skew: boolean;
    version_output?: string | null;
    error?: string | null;
  };
  route?: ServeRouteSnapshot | null;
  routes: ServeRouteSnapshot[];
  conflict?: string | null;
  health: {
    checked_url?: string | null;
    ok: boolean;
    status?: number | null;
    daemon_build?: string | null;
    expected_build?: string | null;
    build_matches?: boolean | null;
    error?: string | null;
  };
}

export interface ServeRouteSnapshot {
  host?: string | null;
  https_port: number;
  proxy?: string | null;
  ownership: "owned" | "conflict" | "free" | "unknown";
}

export type ServeSetupState = "not-installed" | "active" | "port-collision" | "can-enable";

export interface ServeSetupPlan {
  state: ServeSetupState;
  /** Public HTTPS port the setup will request. */
  httpsPort: number;
  /** Exact shell commands the enable will run, in order — shown verbatim at the
   * confirm step so the user sees precisely what mutation they are authorizing. */
  commands: string[];
  /** The clean URL this will produce, when the MagicDNS name is known. */
  resultUrl: string | null;
  /** True only when an enable is safe to offer (state === 'can-enable'). */
  canEnable: boolean;
}

/** Pre-filled prompt for the "Set up with AI" agent: installs Tailscale (if
 * missing) and configures Serve so the web UI gets a clean HTTPS link. */
export function serveSetupTask(status: ServeStatus | null, apiPort: number): string {
  const installed = status?.installed ? "yes" : "no";
  const platform = status?.platform || "unknown";
  return [
    "Set up Tailscale Serve so I can open this CodeTerm over HTTPS from my phone.",
    `Context: tailscale installed = ${installed}, platform = ${platform}, CodeTerm API port = ${apiPort}.`,
    "If Tailscale isn't installed, install it and run `tailscale up` so I'm logged into my tailnet.",
    `Then expose this daemon: \`tailscale serve --bg --https=443 http://127.0.0.1:${apiPort}\` (on macOS provision the cert first with \`tailscale cert <my-magicdns-name>\`).`,
    "Finally verify the HTTPS URL actually responds, then reply with the clean link I can scan.",
  ].join(" ");
}

/** Sibling CodeTerm API port (prod 7685 ↔ dev 7686). Mirrors the backend. */
function siblingCodetermPort(port: number): number | null {
  if (port === 7685) return 7686;
  if (port === 7686) return 7685;
  return null;
}

/** A public HTTPS port that collides with our own or the sibling CodeTerm's API
 * port would proxy into the wrong daemon — refuse it. Mirrors the backend. */
function wouldClobberCodetermPort(apiPort: number, httpsPort: number): boolean {
  return httpsPort === apiPort || siblingCodetermPort(apiPort) === httpsPort;
}

/** Pure decision for the Web Access "Enable" affordance: given the probed Serve
 * status, this daemon's API port, and the host platform, decide which UI state
 * to show and the exact commands an enable would run. No side effects — the
 * mutation only happens when the UI invokes `enable_web_access` after confirm. */
export function planServeSetup(
  status: ServeStatus,
  apiPort: number,
  platform: string,
): ServeSetupPlan {
  const httpsPort = DEFAULT_HTTPS_PORT;
  const isMac = platform === "macos" || platform === "darwin";
  const proxy = `http://127.0.0.1:${apiPort}`;
  const serveCmd = `tailscale serve --bg --https=${httpsPort} ${proxy}`;
  // the MagicDNS name (the cert command's argument).
  const commands =
    isMac && status.self_dns ? [`tailscale cert ${status.self_dns}`, serveCmd] : [serveCmd];
  const suffix = httpsPort === DEFAULT_HTTPS_PORT ? "" : `:${httpsPort}`;
  const resultUrl = status.self_dns ? `https://${status.self_dns}${suffix}/` : null;

  let state: ServeSetupState;
  if (!status.installed) state = "not-installed";
  else if (status.active) state = "active";
  else if (wouldClobberCodetermPort(apiPort, httpsPort)) state = "port-collision";
  else state = "can-enable";

  return { state, httpsPort, commands, resultUrl, canEnable: state === "can-enable" };
}
