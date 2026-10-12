import { parse } from "es-module-lexer/js";
import type { TransportClient } from "./transportClient";

export const MAX_PLUGIN_DOCUMENT_BYTES = 8 * 1024 * 1024;

export interface PluginDocument {
  html: string;
  dispose(): void;
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
  const meta = document.createElement("meta");
  meta.httpEquiv = "Content-Security-Policy";
  meta.content = policy;
  document.head.prepend(meta);
  return bounded("<!doctype html>" + document.documentElement.outerHTML);
}

export async function loadPluginDocument(client: TransportClient, path: string, signal: AbortSignal): Promise<PluginDocument> {
  const request = async (assetPath: string) => {
    const response = await client.request!(assetPath, { signal }, true);
    if (!response.ok) throw new Error(`Plugin asset HTTP ${response.status}`);
    return response;
  };
  const response = await request(path);
  const policy = response.headers.get("Content-Security-Policy");
  if (!policy) throw new Error("Plugin document security policy missing");
  const html = await materializePluginDocument(await response.text(), policy, path, client.origin(), async (assetPath) => {
    const asset = await request(assetPath);
    return { bytes: new Uint8Array(await asset.arrayBuffer()), mime: asset.headers.get("Content-Type")?.split(";")[0].trim() ?? "" };
  });
  signal.throwIfAborted();
  return { html, dispose() {} };
}
