import { describe, expect, it, vi } from "vitest";
import { loadPluginDocument, pluginAssetPath } from "./pluginDocument";
import { createTransportClient } from "./transportClient";

const origin = "http://127.0.0.1:7690";
const path = "/api/plugins/example/ui/index.html?surface=board&instance=own&tab=tab-1";
const policy = "default-src 'none'; script-src 'nonce-fixture'; img-src blob: data:; connect-src 'none'; base-uri 'none'";

function fixture(failAsset = false) {
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async (input) => {
    const pathname = new URL(String(input)).pathname;
    if (pathname.endsWith("index.html")) return new Response('<html><head><style nonce="fixture">body{background:url(./icon.svg)}</style></head><body><img src="./icon.svg"><script nonce="fixture" src="./app.js"></script></body></html>', {
      headers: { "Content-Type": "text/html", "Content-Security-Policy": policy },
    });
    if (pathname.endsWith("app.js")) return new Response("window.test=1", { status: failAsset ? 403 : 200, headers: { "Content-Type": "text/javascript" } });
    return new Response("<svg></svg>", { headers: { "Content-Type": "image/svg+xml" } });
  });
  const blobs: Blob[] = [];
  const objectUrls = { createObjectURL: vi.fn((blob: Blob) => { blobs.push(blob); return `blob:fixture/${blobs.length}`; }), revokeObjectURL: vi.fn() };
  const client = createTransportClient({ origin, token: "secret-fixture", fetch });
  return { fetch, blobs, objectUrls, client };
}

describe("authenticated plugin document", () => {
  it("loads document, icon and bundle through bearer headers and exposes only object URLs", async () => {
    const f = fixture();
    const result = await loadPluginDocument(f.client, path, new AbortController().signal, f.objectUrls);
    const document = new DOMParser().parseFromString(result.html, "text/html");
    expect(document.querySelector("img")?.getAttribute("src")).toBe("blob:fixture/1");
    expect(document.querySelector("script")?.getAttribute("src")).toBe("blob:fixture/2");
    expect(document.querySelector("style")?.textContent).toContain('url("blob:fixture/3")');
    expect(document.querySelector('meta[http-equiv="Content-Security-Policy"]')?.getAttribute("content")).toBe(policy);
    expect(document.querySelector("script")?.getAttribute("nonce")).toBe("fixture");
    for (const [url, init] of f.fetch.mock.calls) {
      expect(String(url)).not.toContain("secret-fixture");
      expect(new URL(String(url)).searchParams.has("token")).toBe(false);
      expect(new Headers(init?.headers).get("authorization")).toBe("Bearer secret-fixture");
      expect(init?.credentials).toBe("omit");
    }
    expect(result.html).not.toContain("secret-fixture");
    result.dispose();
    result.dispose();
    expect(f.objectUrls.revokeObjectURL.mock.calls).toEqual([["blob:fixture/1"], ["blob:fixture/2"], ["blob:fixture/3"]]);
  });
  it("confines asset URLs to the plugin UI directory and rejects token-bearing URLs", () => {
    expect(pluginAssetPath("./icon.svg", path, origin)).toBe("/api/plugins/example/ui/icon.svg");
    for (const reference of ["https://foreign.example/app.js", "/api/config", "../plugin.js", "./app.js?token=secret", "//foreign.example/app.js"]) {
      expect(() => pluginAssetPath(reference, path, origin)).toThrow();
    }
  });
  it("revokes already loaded assets when authentication fails", async () => {
    const f = fixture(true);
    await expect(loadPluginDocument(f.client, path, new AbortController().signal, f.objectUrls)).rejects.toThrow("HTTP 403");
    expect(f.objectUrls.revokeObjectURL).toHaveBeenCalledExactlyOnceWith("blob:fixture/1");
  });
});
