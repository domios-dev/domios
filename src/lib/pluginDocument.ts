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

export interface PluginDocument {
  html: string;
  frameToken: string | null;
  dispose(): void;
  invoke(method: string, args: unknown): Promise<unknown>;
  media(id: string): Promise<PluginMediaHandle>;
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
  const viewLease = response.headers.get("X-Domios-Plugin-View-Frame");
  if (!viewLease || !/^[a-f0-9]{64}$/.test(viewLease)) throw new Error("Plugin view registration missing");
  const frame = response.headers.get("X-Domios-Plugin-Media-Frame");
  const plugin = new URL(path, client.origin()).pathname.match(/^\/api\/plugins\/([a-z0-9_-]{1,128})\/ui\//)?.[1];
  if (frame !== null && (!/^[a-f0-9]{64}$/.test(frame) || !plugin)) throw new Error("Invalid plugin media frame");
  const base = frame && plugin ? `/api/plugins/${plugin}` : null;
  let disposed = false;
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    if (plugin) void client.request!(`/api/plugins/${plugin}/view-frame/${viewLease}`, { method: "DELETE", keepalive: true }, true).catch(() => undefined);
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
    return { html, frameToken: frame, dispose, async invoke(method, args) {
      if (disposed || signal.aborted || !plugin) throw new Error("Plugin view unavailable");
      const result = await client.localJson<unknown>("POST", "/api/plugins/view-call", {pluginId:plugin,viewFrame:viewLease,method,args}, {signal});
      if (disposed || signal.aborted) throw new Error("Plugin view unavailable");
      return result;
    }, async media(id) {
      if (disposed || signal.aborted || !base || typeof id !== "string" || !id || id.length > 256) throw new Error("Plugin media unavailable");
      const result = await client.localJson<PluginMediaHandle>("POST", `${base}/media-register/${frame}`, { id }, { signal });
      if (disposed || signal.aborted) throw new Error("Plugin media unavailable");
      if (!/^[a-f0-9]{64}$/.test(result.handle)) throw new Error("Invalid plugin media handle");
      const expected = `${base}/media/${frame}/${result.handle}`;
      if (result.url !== null && result.url !== expected) throw new Error("Invalid plugin media URL");
      return { ...result, url: result.url === null ? null : new URL(expected, client.origin()).href };
    } };
  } catch (error) { dispose(); throw error; }
}
