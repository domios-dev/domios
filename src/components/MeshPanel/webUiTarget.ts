import type { AppConfig } from "../../lib/types";
import {
  buildWebAccessUrls,
  webLinkWithToken,
  type ServeStatus,
} from "../../lib/webAccess";

export interface NetworkInfo {
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

  const picked = buildWebAccessUrls(network, config.api.port);
  return {
    link: webLinkWithToken(picked.primary, config.api.token),
    usingFallback: picked.usingFallback,
    port,
  };
}

function serveHttpsRoot(serve: ServeStatus | null): string | null {
  const snapshotUrl = serve?.snapshot?.state === "ready" ? serve.snapshot.url : null;
  const url = snapshotUrl ?? (serve?.active ? serve.https_url : null);
  return url ? ensureTrailingSlash(url) : null;
}

function ensureTrailingSlash(url: string): string {
  return url.endsWith("/") ? url : `${url}/`;
}
