import type { TransportClient } from "./transportClient";

export interface PluginDocument {
  html: string;
  dispose(): void;
}

export function pluginAssetPath(reference: string, documentPath: string, origin: string): string {
  const documentUrl = new URL(documentPath, origin);
  const url = new URL(reference, documentUrl);
  if (!/^\/api\/plugins\/[^/]+\/ui\//.test(documentUrl.pathname)) throw new Error("Plugin UI document path required");
  const root = documentUrl.pathname.slice(0, documentUrl.pathname.indexOf("/ui/") + 4);
  if (url.origin !== origin || !url.pathname.startsWith(root) || url.search || url.hash) {
    throw new Error("Plugin asset must stay within its UI directory without credentials");
  }
  return url.pathname;
}

export async function loadPluginDocument(
  client: TransportClient, path: string, signal: AbortSignal,
  objectUrls: Pick<typeof URL, "createObjectURL" | "revokeObjectURL"> = URL,
): Promise<PluginDocument> {
  const urls: string[] = [];
  const dispose = () => urls.splice(0).forEach((url) => objectUrls.revokeObjectURL(url));
  const request = async (assetPath: string) => {
    const response = await client.request!(assetPath, { signal }, true);
    if (!response.ok) throw new Error(`Plugin asset HTTP ${response.status}`);
    return response;
  };
  const objectUrl = (blob: Blob) => {
    const url = objectUrls.createObjectURL(blob);
    urls.push(url);
    return url;
  };
  try {
    const response = await request(path);
    const document = new DOMParser().parseFromString(await response.text(), "text/html");
    const asset = async (reference: string, parentPath = path): Promise<string> => {
      const assetPath = pluginAssetPath(reference, parentPath, client.origin());
      const response = await request(assetPath);
      if (response.headers.get("Content-Type")?.startsWith("text/css")) {
        return objectUrl(new Blob([await cssAssets(await response.text(), assetPath)], { type: "text/css" }));
      }
      return objectUrl(await response.blob());
    };
    const cssAssets = async (css: string, parentPath: string) => {
      const references = [...css.matchAll(/url\(\s*(["']?)([^"')]+)\1\s*\)/g)];
      for (const match of references) {
        const reference = match[2].trim();
        if (reference.startsWith("data:") || reference.startsWith("#")) continue;
        css = css.replace(match[0], `url("${await asset(reference, parentPath)}")`);
      }
      return css;
    };
    for (const element of document.querySelectorAll("script[src],img[src],link[href]")) {
      const attribute = element.hasAttribute("src") ? "src" : "href";
      const reference = element.getAttribute(attribute)!;
      if (reference.startsWith("data:")) continue;
      element.setAttribute(attribute, await asset(reference));
    }
    for (const style of document.querySelectorAll("style")) {
      style.textContent = await cssAssets(style.textContent ?? "", path);
    }
    const policy = response.headers.get("Content-Security-Policy");
    if (!policy) throw new Error("Plugin document security policy missing");
    {
      const meta = document.createElement("meta");
      meta.httpEquiv = "Content-Security-Policy";
      meta.content = policy;
      document.head.prepend(meta);
    }
    signal.throwIfAborted();
    return { html: "<!doctype html>" + document.documentElement.outerHTML, dispose };
  } catch (error) {
    dispose();
    throw error;
  }
}
