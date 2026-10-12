// @vitest-environment-options {"happyDOM":{"settings":{"disableJavaScriptFileLoading":true,"disableCSSFileLoading":true}}}
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Window } from "happy-dom";
import { loadPluginDocument, materializePluginDocument, MAX_PLUGIN_DOCUMENT_BYTES, pluginAssetPath } from "./pluginDocument";
import { createTransportClient } from "./transportClient";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import ts from "typescript";

const origin = "http://127.0.0.1:7690";
const path = "/api/plugins/example/ui/index.html?surface=board&instance=own&tab=tab-1";
const policy = "default-src 'none'; script-src 'nonce-fixture'; style-src 'unsafe-inline'; img-src blob: data:; font-src blob: data:; connect-src 'none'; base-uri 'none'; form-action 'none'";

let parserWindow: Window;
beforeEach(() => {
  parserWindow = new Window({ settings: { disableJavaScriptFileLoading: true, disableCSSFileLoading: true, handleDisabledFileLoadingAsSuccess: true } });
  vi.stubGlobal("DOMParser", parserWindow.DOMParser);
});
afterEach(async () => {
  vi.unstubAllGlobals();
  await parserWindow.close();
});

function resourceMarkup(html: string): string {
  const document = new DOMParser().parseFromString(html, "text/html");
  document.querySelector('meta[http-equiv="Content-Security-Policy"]')?.remove();
  return document.documentElement.outerHTML;
}

function fixture(assetStatus = 200) {
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async (input) => {
    const pathname = new URL(String(input)).pathname;
    if (pathname.endsWith("index.html")) return new Response('<html><head><style nonce="fixture">body{background:url(./icon.svg)}</style></head><body><img src="./icon.svg"><script nonce="fixture" src="./app.js"></script></body></html>', {
      headers: { "Content-Type": "text/html", "Content-Security-Policy": policy },
    });
    if (pathname.endsWith("app.js")) return new Response("window.test=1", { status: assetStatus, headers: { "Content-Type": "text/javascript" } });
    return new Response("<svg></svg>", { headers: { "Content-Type": "image/svg+xml" } });
  });
  const client = createTransportClient({ origin, token: "secret-fixture", fetch });
  return { fetch, client };
}

describe("authenticated plugin document", () => {
  it("keeps inline and fetched scripts in order, preserves modules, and escapes script closing markup", async () => {
    const raw = '<html><head><script nonce="fixture">window.first=1</script></head><body><script nonce="fixture" src="./classic.js" defer></script><script nonce="fixture" type="module" src="./module.js"></script></body></html>';
    const assets = new Map([
      ["/api/plugins/example/ui/classic.js", 'window.second="</script><div>not markup</div>"'],
      ["/api/plugins/example/ui/module.js", "export const third = await Promise.resolve(3);"],
    ]);
    const html = await materializePluginDocument(raw, policy, path, origin, async (path) => ({
      bytes: new TextEncoder().encode(assets.get(path)!), mime: "text/javascript",
    }));
    const document = new DOMParser().parseFromString(html, "text/html");
    const scripts = [...document.querySelectorAll("script")];
    expect(scripts.map((script) => script.textContent)).toEqual([
      "window.first=1", 'window.second="<\\/script><div>not markup</div>"', "export const third = await Promise.resolve(3);",
    ]);
    expect(scripts.map((script) => script.getAttribute("nonce"))).toEqual(["fixture", "fixture", "fixture"]);
    expect(scripts.map((script) => script.getAttribute("type"))).toEqual([null, null, "module"]);
    expect(document.querySelectorAll("script[src],div").length).toBe(0);
  });

  it.each([
    'import "./chunk.js";', 'export { value } from "./chunk.js";', 'import("./chunk.js");', 'import(chunk);', "console.log(import.meta.url);",
  ])("rejects module dependency semantics that cannot be kept in srcDoc: %s", async (source) => {
    const raw = '<script nonce="fixture" type="module" src="./module.js"></script>';
    await expect(materializePluginDocument(raw, policy, path, origin, async () => ({
      bytes: new TextEncoder().encode(source), mime: "text/javascript",
    }))).rejects.toThrow("self-contained");
  });

  it("does not confuse import text inside strings or comments with chunk dependencies", async () => {
    const raw = '<script nonce="fixture">const text = "import(whatever)"; /* import "./chunk.js" */</script>';
    await expect(materializePluginDocument(raw, policy, path, origin, vi.fn())).resolves.toContain('"import(whatever)"');
  });

  it("preserves mixed-case closing-tag strings and escapes HTML parser comment transitions", async () => {
    const raw = '<script nonce="fixture" src="./app.js"></script>';
    const read = async (source: string) => ({ bytes: new TextEncoder().encode(source), mime: "text/javascript" });
    const html = await materializePluginDocument(raw, policy, path, origin, async () => read('const text="</ScRiPt>";'));
    expect(new DOMParser().parseFromString(html, "text/html").querySelector("script")?.textContent).toBe('const text="<\\/ScRiPt>";');
    const comments = await materializePluginDocument(raw, policy, path, origin, async () => read('const text="<!--<script>"; const pattern=/<!--/;'));
    expect(new DOMParser().parseFromString(comments, "text/html").querySelector("script")?.textContent).toBe('const text="\\x3c!--<script>"; const pattern=/\\x3c!--/;');
  });

  it.each(['<script src="./app.js"></script>', '<script nonce="wrong" src="./app.js"></script>'])("rejects scripts without the response nonce", async (raw) => {
    const read = vi.fn();
    await expect(materializePluginDocument(raw, policy, path, origin, read)).rejects.toThrow("nonce");
    expect(read).not.toHaveBeenCalled();
  });

  it("inlines linked CSS and materializes its font and image URLs without network sources", async () => {
    const raw = '<link rel="stylesheet" href="./nested/style.css">';
    const read = vi.fn(async (path: string) => path.endsWith(".css")
      ? { bytes: new TextEncoder().encode('@font-face{src:url(./font.woff2)}body{background:url(../icon.svg)}'), mime: "text/css" }
      : { bytes: new Uint8Array([1, 2, 3]), mime: path.endsWith(".woff2") ? "font/woff2" : "image/svg+xml" });
    const html = await materializePluginDocument(raw, policy, path, origin, read);
    expect(html).toContain("data:font/woff2;base64,AQID");
    expect(html).toContain("data:image/svg+xml;base64,AQID");
    expect(resourceMarkup(html)).not.toContain("blob:");
    expect(new DOMParser().parseFromString(html, "text/html").querySelector("link")).toBeNull();
    expect(read.mock.calls.map(([path]) => path)).toEqual([
      "/api/plugins/example/ui/nested/style.css", "/api/plugins/example/ui/nested/font.woff2", "/api/plugins/example/ui/icon.svg",
    ]);
  });

  it("rejects oversized inputs and data URL expansion beyond the document size bound", async () => {
    await expect(materializePluginDocument("x".repeat(MAX_PLUGIN_DOCUMENT_BYTES + 1), policy, path, origin, vi.fn())).rejects.toThrow("8 MiB");
    await expect(materializePluginDocument('<img src="./huge.png">', policy, path, origin, async () => ({
      bytes: new Uint8Array(MAX_PLUGIN_DOCUMENT_BYTES * 0.8), mime: "image/png",
    }))).rejects.toThrow("8 MiB");
  });

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
    const loaded = await loadPluginDocument(client, `/api/plugins/${id}/ui/index.html`, new AbortController().signal);
    const original = new DOMParser().parseFromString(raw, "text/html");
    const prepared = new DOMParser().parseFromString(loaded.html, "text/html");
    expect(prepared.querySelector("style")?.textContent).toBe(original.querySelector("style")?.textContent);
    expect(prepared.querySelectorAll("style").length).toBeGreaterThan(0);
    expect(prepared.querySelectorAll('link[rel="stylesheet"]').length).toBe(0);
    expect(prepared.querySelector('meta[http-equiv="Content-Security-Policy"]')?.getAttribute("content")).toBe(policy);
    const script = prepared.querySelector("script")!;
    const source = readFileSync(resolve(directory, original.querySelector("script")!.getAttribute("src")!), "utf8");
    expect(script.hasAttribute("src")).toBe(false);
    expect(script.textContent).toBe(source.replace(/<!--/g, "\\x3c!--").replace(/<\/script/gi, (closing) => `<\\/${closing.slice(2)}`));
    expect(script.getAttribute("nonce")).toBe("fixture");
    expect(resourceMarkup(loaded.html)).not.toContain("blob:");
    expect(new TextEncoder().encode(loaded.html).byteLength).toBeLessThan(MAX_PLUGIN_DOCUMENT_BYTES);
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

  it("loads document and assets through bearer headers and materializes scripts and images", async () => {
    const f = fixture();
    const result = await loadPluginDocument(f.client, path, new AbortController().signal);
    const document = new DOMParser().parseFromString(result.html, "text/html");
    expect(document.querySelector("img")?.getAttribute("src")).toMatch(/^data:image\/svg\+xml;base64,/);
    expect(document.querySelector("script")?.hasAttribute("src")).toBe(false);
    expect(document.querySelector("script")?.textContent).toBe("window.test=1");
    expect(document.querySelector("style")?.textContent).toContain('url("data:image/svg+xml;base64,');
    expect(resourceMarkup(result.html)).not.toContain("blob:");
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

  });
  it("confines asset URLs to the plugin UI directory and rejects token-bearing URLs", () => {
    expect(pluginAssetPath("./icon.svg", path, origin)).toBe("/api/plugins/example/ui/icon.svg");
    for (const reference of ["https://foreign.example/app.js", "/api/config", "../plugin.js", "./app.js?token=secret", "//foreign.example/app.js"]) {
      expect(() => pluginAssetPath(reference, path, origin)).toThrow();
    }
  });
  it.each([403, 503, 429])("reports asset HTTP %s instead of returning a partially materialized frame", async (status) => {
    const f = fixture(status);
    await expect(loadPluginDocument(f.client, path, new AbortController().signal)).rejects.toThrow(`HTTP ${status}`);

  });
});
