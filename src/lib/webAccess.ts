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
 * URLs are bare (end in /web/). Append the API token as a fragment with
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
  const https = net?.tailscale_https_url ? `${net.tailscale_https_url}/web/` : null;
  const list = [
    https,
    net?.tailscale_ip ? `http://${net.tailscale_ip}:${port}/web/` : null,
    net?.local_ip ? `http://${net.local_ip}:${port}/web/` : null,
    `http://127.0.0.1:${port}/web/`,
  ].filter((v): v is string => Boolean(v));
  const urls = [...new Set(list)];
  return { primary: urls[0], urls, usingFallback: !https };
}

/** Append the API token as a URL fragment (never a query — fragments aren't sent
 * to the server and survive the FQDN root form). */
export function webLinkWithToken(url: string, token?: string | null): string {
  return token ? `${url}#token=${encodeURIComponent(token)}` : url;
}

/** Curriculum hint shown when falling back to an IP+port URL: the one command
 * that turns a raw IP link into a clean, phone-friendly HTTPS URL. */
export function serveHint(port: number): string {
  return `Tip: run 'tailscale serve --bg --https=443 http://127.0.0.1:${port}' for a clean HTTPS URL — phones prefer it.`;
}
