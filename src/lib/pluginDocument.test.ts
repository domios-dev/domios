// @vitest-environment-options {"happyDOM":{"settings":{"disableJavaScriptFileLoading":true,"disableCSSFileLoading":true}}}
import { describe, expect, it, vi } from "vitest";
import { loadPluginDocument, pluginAssetPath } from "./pluginDocument";
import { createTransportClient } from "./transportClient";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import ts from "typescript";

const origin = "http://127.0.0.1:7690";
const path = "/api/plugins/example/ui/index.html?surface=board&instance=own&tab=tab-1";
const policy = "default-src 'none'; script-src 'nonce-fixture'; style-src 'unsafe-inline'; img-src blob: data:; font-src blob: data:; connect-src 'none'; base-uri 'none'; form-action 'none'";

function fixture(assetStatus = 200) {
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async (input) => {
    const pathname = new URL(String(input)).pathname;
    if (pathname.endsWith("index.html")) return new Response('<html><head><style nonce="fixture">body{background:url(./icon.svg)}</style></head><body><img src="./icon.svg"><script nonce="fixture" src="./app.js"></script></body></html>', {
      headers: { "Content-Type": "text/html", "Content-Security-Policy": policy },
    });
    if (pathname.endsWith("app.js")) return new Response("window.test=1", { status: assetStatus, headers: { "Content-Type": "text/javascript" } });
    return new Response("<svg></svg>", { headers: { "Content-Type": "image/svg+xml" } });
  });
  const blobs: Blob[] = [];
  const objectUrls = { createObjectURL: vi.fn((blob: Blob) => { blobs.push(blob); return `blob:fixture/${blobs.length}`; }), revokeObjectURL: vi.fn() };
  const client = createTransportClient({ origin, token: "secret-fixture", fetch });
  return { fetch, blobs, objectUrls, client };
}

describe("authenticated plugin document", () => {
  it.each(["analytics", "git"])("keeps the real %s bundle's stylesheet authorized through srcDoc preparation", async (id) => {
    const directory = resolve("plugins", id, "ui");
    const raw = readFileSync(resolve(directory, "index.html"), "utf8").replaceAll("__CT_NONCE__", "fixture");
    const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async (input) => {
      const filename = new URL(String(input)).pathname.split("/").pop()!;
      return new Response(filename === "index.html" ? raw : readFileSync(resolve(directory, filename), "utf8"), {
        headers: filename === "index.html"
          ? { "Content-Type": "text/html", "Content-Security-Policy": policy }
          : { "Content-Type": "text/javascript" },
      });
    });
    const client = createTransportClient({ origin, token: "fixture", fetch });
    const objectUrls = { createObjectURL: vi.fn(() => "blob:fixture/bundle"), revokeObjectURL: vi.fn() };
    const loaded = await loadPluginDocument(client, `/api/plugins/${id}/ui/index.html`, new AbortController().signal, objectUrls);
    const original = new DOMParser().parseFromString(raw, "text/html");
    const prepared = new DOMParser().parseFromString(loaded.html, "text/html");
    expect(prepared.querySelector("style")?.textContent).toBe(original.querySelector("style")?.textContent);
    expect(prepared.querySelectorAll("style").length).toBeGreaterThan(0);
    expect(prepared.querySelectorAll('link[rel="stylesheet"]').length).toBe(0);
    expect(prepared.querySelector('meta[http-equiv="Content-Security-Policy"]')?.getAttribute("content")).toBe(policy);
    expect(fetch).toHaveBeenCalledTimes(2);
    loaded.dispose();
  });

  it("authorizes the real Git diff bundle's runtime stylesheet elements without authorizing external CSS", () => {
    const directory = resolve("plugins", "git", "ui");
    const html = new DOMParser().parseFromString(readFileSync(resolve(directory, "index.html"), "utf8"), "text/html");
    const bundle = readFileSync(resolve(directory, html.querySelector("script[src]")!.getAttribute("src")!), "utf8");
    const source = ts.createSourceFile("git.js", bundle, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
    const styles: ts.ObjectLiteralExpression[] = [];
    const visit = (node: ts.Node) => {
      if (ts.isCallExpression(node) && node.arguments.length > 1
        && ts.isStringLiteral(node.arguments[0]) && node.arguments[0].text === "style"
        && ts.isObjectLiteralExpression(node.arguments[1])) styles.push(node.arguments[1]);
      ts.forEachChild(node, visit);
    };
    visit(source);
    expect(styles.length).toBeGreaterThan(0);
    expect(styles.some((props) => !props.properties.some((prop) => prop.name?.getText(source) === "nonce"))).toBe(true);
    const styleSources = policy.split("; ").find((directive) => directive.startsWith("style-src "))!.split(" ").slice(1);
    expect(styleSources).toEqual(["'unsafe-inline'"]);
  });

  it("loads document, icon and bundle through bearer headers and exposes only object URLs", async () => {
    const f = fixture();
    const result = await loadPluginDocument(f.client, path, new AbortController().signal, f.objectUrls);
    const document = new DOMParser().parseFromString(result.html, "text/html");
    expect(document.querySelector("img")?.getAttribute("src")).toBe("blob:fixture/1");
    expect(document.querySelector("script")?.getAttribute("src")).toBe("blob:fixture/2");
    expect(document.querySelector("style")?.textContent).toContain('url("blob:fixture/3")');
    expect(document.querySelector('meta[http-equiv="Content-Security-Policy"]')?.getAttribute("content")).toBe(policy);
    expect(document.querySelector("script")?.getAttribute("nonce")).toBe("fixture");
    expect(document.querySelector("style")?.getAttribute("nonce")).toBe("fixture");
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
  it.each([403, 503, 429])("reports asset HTTP %s and revokes already loaded assets", async (status) => {
    const f = fixture(status);
    await expect(loadPluginDocument(f.client, path, new AbortController().signal, f.objectUrls)).rejects.toThrow(`HTTP ${status}`);
    expect(f.objectUrls.revokeObjectURL).toHaveBeenCalledExactlyOnceWith("blob:fixture/1");
  });
});
