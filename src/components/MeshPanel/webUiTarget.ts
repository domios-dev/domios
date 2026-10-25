import type { AppConfig } from "../../lib/types";
import {
  buildWebAccessUrls,
  webLinkWithToken,
  type ServeStatus,
} from "../../lib/webAccess";

import type { NetworkTransportStatus } from "./networkTransport";

export interface NetworkInfo {
  web_listener?: string | null;
  transport?: NetworkTransportStatus | null;
  local_ip?: string | null;
  tailscale_ip?: string | null;
  tailscale_https_url?: string | null;
  api_port?: number;
}

export interface WebUiTarget {
  link: string;
  usingFallback: boolean;
  port: number;
}

export type WebRouteKind = "tailscale_https" | "tailscale_ip" | "lan" | "loopback";

export function displayedWebUrl(link: string): string {
  const url = new URL(link);
  url.hash = "";
  url.searchParams.delete("token");
  return url.toString();
}

export function classifyWebUiRoute(config: AppConfig, network: NetworkInfo | null, serve: ServeStatus | null = null): { kind: WebRouteKind; reason: string } {
  const url = new URL(buildWebUiTarget(config, network, serve).link);
  const host = url.hostname.toLowerCase();
  if (host === "localhost" || host === "127.0.0.1" || host === "[::1]") {
    return { kind: "loopback", reason: "Local access on this machine only. A phone cannot reach this loopback address." };
  }
  if (url.protocol === "https:") {
    return { kind: "tailscale_https", reason: "HTTPS through this machine's Tailscale Serve gateway. Connect your phone to the same tailnet." };
  }
  const parts = host.split(".").map(Number);
  if (host.startsWith("[fd7a:115c:a1e0:") || (parts.length === 4 && parts[0] === 100 && parts[1] >= 64 && parts[1] < 128)) {
    return { kind: "tailscale_ip", reason: "Direct HTTP over this machine's Tailscale address. Connect your phone to the same tailnet; HTTPS Serve is not selected." };
  }
  return { kind: "lan", reason: "Direct HTTP over the local network. Your phone must be on the same LAN; HTTPS Serve is not selected." };
}

export function buildWebUiTarget(
  config: AppConfig,
  network: NetworkInfo | null,
  serve: ServeStatus | null = null,
): WebUiTarget {
  const port = network?.api_port ?? serve?.snapshot?.api_port ?? config.api.port;
  const serveRoot = serveHttpsRoot(serve);
  if (serveRoot) {
    return {
      link: webLinkWithToken(serveRoot, config.api.token),
      usingFallback: false,
      port,
    };
  }

  if (network?.web_listener) {
    const root = `http://${network.web_listener}/`;
    return { link: webLinkWithToken(root, config.api.token), usingFallback: true, port: Number(new URL(root).port) || port };
  }
  const picked = buildWebAccessUrls(network, config.api.port);
  return {
    link: webLinkWithToken(picked.primary, config.api.token),
    usingFallback: picked.usingFallback,
    port,
  };
}

function serveHttpsRoot(serve: ServeStatus | null): string | null {
  const snapshot = serve?.snapshot;
  return snapshot?.state === "ready" && snapshot.health.ok && snapshot.url
    ? ensureTrailingSlash(snapshot.url) : null;
}

function ensureTrailingSlash(url: string): string {
  return url.endsWith("/") ? url : `${url}/`;
}

export function shouldShowWebSetupHelper(route: WebRouteKind, ready: boolean): boolean {
  return !ready || route !== "tailscale_https";
}

export function webRouteLabel(route: WebRouteKind): string {
  return { tailscale_https: "HTTPS · Tailscale", tailscale_ip: "HTTP · Tailscale", lan: "Local network", loopback: "This machine" }[route];
}

export function webUiListenerReady(network: NetworkInfo | null, serve: ServeStatus | null): boolean {
  if (network?.transport && network.transport.state !== "connected") return false;
  return Boolean(network?.web_listener || serveHttpsRoot(serve));
}
