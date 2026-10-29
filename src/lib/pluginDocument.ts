import { PluginViewChannel } from "./pluginViewChannel";
import { PLUGIN_UI_MONITOR } from "./pluginUiMonitor";
import { parse } from "es-module-lexer/js";
import type { TransportClient } from "./transportClient";

export const MAX_PLUGIN_DOCUMENT_BYTES = 8 * 1024 * 1024;

export interface PluginMediaHandle {
  handle: string;
  url: string | null;
  content_type: string;
  file_name: string;
  expires_in_seconds: number;
}

/** The only routes a document's host actions reach; each is owner-only and lease-checked by the daemon. */
export const PLUGIN_HOST_ROUTES = ["/api/plugins/view-host-action", "/api/plugins/view-host-action/download", "/api/plugins/view-input"] as const;
export type PluginHostRoute = (typeof PLUGIN_HOST_ROUTES)[number];

export interface PluginHostResponse {
  response: Response;
  /** Whether the lease the request carried is still this document's; recheck after reading the body. */
  current(): boolean;
}

export interface PluginDocument {
  html: string;
  channel: PluginViewChannel;
  frameToken: string | null;
  dispose(): void;
  invoke(method: string, args: unknown): Promise<unknown>;
  media(id: string): Promise<PluginMediaHandle>;
  /** App-only: never reachable from the frame, which can choose neither the route nor the lease. */
  hostRequest(route: PluginHostRoute, init: { body: BodyInit; headers?: Record<string, string>; signal?: AbortSignal }): Promise<PluginHostResponse>;
}

export interface PluginAsset {
  bytes: Uint8Array;
  mime: string;
}

export function pluginAssetPath(reference: string, documentPath: string, origin: string): string {
  const documentUrl = new URL(documentPath, origin);
  const url = new URL(reference, documentUrl);
  if (!/^\/api\/plugins\/[^/]+\/ui\//.test(documentUrl.pathname)) throw new Error("Plugin UI document path required");
  const root = documentUrl.pathname.slice(0, documentUrl.pathname.indexOf("/ui/") + 4);
  if (url.origin !== origin || !url.pathname.startsWith(root) || url.search || url.hash || url.username || url.password) {
    throw new Error("Plugin asset must stay within its UI directory without credentials");
  }
  return url.pathname;
}

function bounded(text: string): string {
  if (new TextEncoder().encode(text).byteLength > MAX_PLUGIN_DOCUMENT_BYTES) {
    throw new Error("Plugin document exceeds 8 MiB limit");
  }
  return text;
}

function dataUrl(asset: PluginAsset): string {
  if (!/^(image\/|font\/)/.test(asset.mime)) throw new Error("Plugin data assets must be images or fonts");
  let binary = "";
  for (let offset = 0; offset < asset.bytes.length; offset += 32768) {
    binary += String.fromCharCode(...asset.bytes.subarray(offset, offset + 32768));
  }
  return `data:${asset.mime};base64,${btoa(binary)}`;
}

export async function materializePluginDocument(
  raw: string, policy: string, path: string, origin: string,
  readAsset: (path: string) => Promise<PluginAsset>,
): Promise<string> {
  const scripts = policy.split(";").map((directive) => directive.trim()).find((directive) => directive.startsWith("script-src "));
  const nonce = scripts?.match(/^script-src 'nonce-([A-Za-z0-9+/_=-]+)'$/)?.[1];
  if (!nonce) throw new Error("Plugin document nonce security policy required");
  const document = new DOMParser().parseFromString(bounded(raw), "text/html");
  let totalBytes = new TextEncoder().encode(raw).byteLength;
  const asset = async (reference: string, parentPath = path) => {
    const result = await readAsset(pluginAssetPath(reference, parentPath, origin));
    totalBytes += result.bytes.byteLength;
    if (totalBytes > MAX_PLUGIN_DOCUMENT_BYTES) throw new Error("Plugin document exceeds 8 MiB limit");
    return result;
  };
  const cssAssets = async (css: string, parentPath: string) => {
    if (/@import\b/i.test(css)) throw new Error("Plugin stylesheets must be self-contained (no @import)");
    for (const match of [...css.matchAll(/url\(\s*(["']?)([^"')]+)\1\s*\)/g)]) {
      const reference = match[2].trim();
      if (reference.startsWith("data:") || reference.startsWith("#")) continue;
      css = css.replace(match[0], `url("${dataUrl(await asset(reference, parentPath))}")`);
    }
    return css;
  };
  for (const script of document.querySelectorAll("script")) {
    if ((script.nonce || script.getAttribute("nonce")) !== nonce) throw new Error("Plugin script nonce does not match document policy");
    const reference = script.getAttribute("src");
    const source = reference === null ? script.textContent ?? "" : new TextDecoder().decode((await asset(reference)).bytes);
    const [imports] = await parse(source);
    if (imports.length) throw new Error("Plugin scripts must be self-contained (no chunk imports or import.meta)");
    // Raw-text HTML parsing must not let a JavaScript string close its script element.
    script.textContent = source.replace(/<!--/g, "\\x3c!--").replace(/<\/script/gi, (closing) => `<\\/${closing.slice(2)}`);
    script.setAttribute("nonce", nonce);
    script.removeAttribute("src");
    script.removeAttribute("integrity");
    script.removeAttribute("crossorigin");
    script.removeAttribute("async");
    script.removeAttribute("defer");
  }
  for (const image of document.querySelectorAll("img[src]")) {
    const reference = image.getAttribute("src")!;
    if (!reference.startsWith("data:")) image.setAttribute("src", dataUrl(await asset(reference)));
  }
  for (const link of document.querySelectorAll("link[href]")) {
    if (link.getAttribute("rel")?.toLowerCase() !== "stylesheet") throw new Error("Unsupported plugin document link");
    const reference = link.getAttribute("href")!;
    const parentPath = pluginAssetPath(reference, path, origin);
    const stylesheet = await asset(reference);
    if (stylesheet.mime !== "text/css") throw new Error("Plugin stylesheet MIME required");
    const style = document.createElement("style");
    if (link.hasAttribute("media")) style.setAttribute("media", link.getAttribute("media")!);
    style.textContent = await cssAssets(new TextDecoder().decode(stylesheet.bytes), parentPath);
    link.replaceWith(style);
  }
  for (const style of document.querySelectorAll("style")) {
    style.textContent = await cssAssets(style.textContent ?? "", path);
  }
  const monitor = document.createElement("script");
  monitor.setAttribute("nonce", nonce);
  monitor.setAttribute("data-plugin-ui-monitor", "");
  monitor.textContent = PLUGIN_UI_MONITOR;
  document.head.prepend(monitor);
  const meta = document.createElement("meta");
  meta.httpEquiv = "Content-Security-Policy";
  meta.content = policy;
  document.head.prepend(meta);
  return bounded("<!doctype html>" + document.documentElement.outerHTML);
}

export async function loadPluginDocument(client: TransportClient, path: string, signal: AbortSignal): Promise<PluginDocument> {
  const request = async (assetPath: string) => {
    const response = await client.request!(assetPath, { signal }, true);
    if (!response.ok) {
      const reason = (await response.text()).slice(0, 1000).trim();
      throw new Error(`Plugin asset HTTP ${response.status} (${assetPath})${reason ? `: ${reason}` : ""}`);
    }
    return response;
  };
  const response = await request(path);
  const policy = response.headers.get("Content-Security-Policy");
  if (!policy) throw new Error("Plugin document security policy missing");
  let viewLease = response.headers.get("X-Domios-Plugin-View-Frame");
  if (!viewLease || !/^[a-f0-9]{64}$/.test(viewLease)) throw new Error("Plugin view registration missing");
  const documentId = response.headers.get("X-Domios-Plugin-View-Document");
  if (!documentId || !/^[a-f0-9]{32}$/.test(documentId)) throw new Error("Plugin document identity missing");
  const frame = response.headers.get("X-Domios-Plugin-Media-Frame");
  const plugin = new URL(path, client.origin()).pathname.match(/^\/api\/plugins\/([a-z0-9_-]{1,128})\/ui\//)?.[1];
  if (frame !== null && (!/^[a-f0-9]{64}$/.test(frame) || !plugin)) throw new Error("Invalid plugin media frame");
  const base = frame && plugin ? `/api/plugins/${plugin}` : null;
  const snapshotText = response.headers.get("X-Domios-Plugin-View-Registration");
  if (!snapshotText || snapshotText.length > 16384 || !plugin) throw new Error("Plugin registration scope missing");
  const snapshot: {registration: string; scope: Record<string, unknown>} = JSON.parse(snapshotText);
  if (!/^[a-f0-9]{64}$/.test(snapshot.registration) || !snapshot.scope || typeof snapshot.scope !== "object" || (frame !== null && frame !== snapshot.registration)) {
    throw new Error("Invalid plugin registration scope");
  }
  let disposed = false;
  let retirementGeneration=0;
  let lifecycle: Promise<unknown> = Promise.resolve();
  const enqueue = (work: () => Promise<void>) => {
    const result = lifecycle.then(work);
    lifecycle = result.catch(() => undefined);
    return result;
  };
  const retire = () => {
    retirementGeneration+=1;
    viewLease=null;
    return enqueue(async () => {
    const response = await client.request!("/api/plugins/view-frame/revoke", {
      method: "POST", headers: {"Content-Type":"application/json"},
      body: JSON.stringify({pluginId:plugin,registration:snapshot.registration}), keepalive:true,
    }, true);
    if (!response.ok) throw new Error("Plugin document retirement failed");
    });
  };
  const channel = new PluginViewChannel(documentId, {
    retire,
    register: () => {
      const capturedGeneration=retirementGeneration;
      return enqueue(async () => {
      if (disposed || signal.aborted) throw new Error("Plugin view unavailable");
      const result = await client.localJson<{viewLease:string}>("POST", "/api/plugins/view-frame/register", {pluginId:plugin,...snapshot}, {signal});
      if (!/^[a-f0-9]{64}$/.test(result.viewLease)) throw new Error("Invalid plugin view registration");
      if (disposed || signal.aborted || capturedGeneration!==retirementGeneration) throw new Error("Plugin view registration retired");
      viewLease = result.viewLease;
      });
    },
  });
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    channel.dispose();
    void retire().catch(() => undefined);
  };
  try {
    const mediaPolicy = base
      ? policy.replace(/(?:^|;\s*)media-src 'none'(?=;|$)/, (directive) => directive.replace("'none'", `${new URL(`${base}/media/${frame}/`, client.origin()).href} blob:`))
      : policy;
    if (base && mediaPolicy === policy) throw new Error("Plugin media security policy missing");
    const html = await materializePluginDocument(await response.text(), mediaPolicy, path, client.origin(), async (assetPath) => {
      const asset = await request(assetPath);
      return { bytes: new Uint8Array(await asset.arrayBuffer()), mime: asset.headers.get("Content-Type")?.split(";")[0].trim() ?? "" };
    });
    signal.throwIfAborted();
    return { html, channel, frameToken: frame, dispose, async invoke(method, args) {
      if (disposed || signal.aborted || !plugin || !viewLease) throw new Error("Plugin view unavailable");
      const capturedLease=viewLease;
      const result = await client.localJson<unknown>("POST", "/api/plugins/view-call", {pluginId:plugin,viewFrame:capturedLease,method,args}, {signal});
      if (disposed || signal.aborted || !viewLease || capturedLease!==viewLease) throw new Error("Plugin view unavailable");
      return result;
    }, async media(id) {
      if (disposed || signal.aborted || !base || !viewLease || typeof id !== "string" || !id || id.length > 256) throw new Error("Plugin media unavailable");
      const capturedLease=viewLease;
      const result = await client.localJson<PluginMediaHandle>("POST", `${base}/media-register/${frame}`, { id,viewFrame:capturedLease }, { signal });
      if (disposed || signal.aborted || !viewLease || capturedLease!==viewLease) throw new Error("Plugin media unavailable");
      if (!/^[a-f0-9]{64}$/.test(result.handle)) throw new Error("Invalid plugin media handle");
      const expected = `${base}/media/${frame}/${result.handle}`;
      if (result.url !== null && result.url !== expected) throw new Error("Invalid plugin media URL");
      return { ...result, url: result.url === null ? null : new URL(expected, client.origin()).href };
    }, async hostRequest(route, init) {
      // The private lease travels as a request header, captured now; a response for a lease that was
      // retired or reminted meanwhile is refused here, and callers recheck after reading the body.
      if (disposed || signal.aborted || !plugin || !viewLease || !PLUGIN_HOST_ROUTES.includes(route)) throw new Error("Plugin view unavailable");
      const capturedLease = viewLease;
      const current = () => !disposed && !signal.aborted && viewLease !== null && viewLease === capturedLease;
      // The registration key is non-secret; the daemon accepts it only with a lease that resolves to it.
      const headers = { ...init.headers, "X-Domios-Plugin-View-Frame": capturedLease, "X-Domios-Plugin-Media-Frame": snapshot.registration };
      const response = await client.request!(route, { method: "POST", headers, body: init.body, signal: init.signal ?? signal }, true);
      if (!current()) throw new Error("Plugin view unavailable");
      return { response, current };
    } };
  } catch (error) { dispose(); throw error; }
}
