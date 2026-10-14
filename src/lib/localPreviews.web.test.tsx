import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AttachmentCapsule, configureHtmlFileLoader, configureLocalArtifactLoader, configureLocalArtifactOpener, LocalArtifactBlock, LocalArtifactScope, RichMarkdown } from "@codeterm/chat-ui";
import { ChangeFileView } from "../components/Terminal/tabPanel/ChangeFileView";
import { DocumentDetail } from "../components/Terminal/tabPanel/DocumentDetail";
import { configureLocalPreviews } from "./configureLocalPreviews";
import { invoke, IS_WEB } from "./bridge";
import { selfHost, setTokenFor } from "./webApi";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let root: Root | null = null;
let host: HTMLDivElement;
let fetchMock: ReturnType<typeof vi.fn>;
const requests: { path: string; body: unknown }[] = [];

beforeEach(() => {
  expect(IS_WEB).toBe(true);
  requests.length = 0;
  localStorage.clear();
  setTokenFor(selfHost(), "preview-fixture-token");
  configureLocalPreviews();
  fetchMock = vi.fn(async (input: string, init?: RequestInit) => {
    expect(new Headers(init?.headers).get("Authorization")).toBe("Bearer preview-fixture-token");
    expect(init?.method).toBe("POST");
    const body = JSON.parse(String(init?.body)) as { path?: string; kind?: string; paths?: string[] };
    const path = new URL(input).pathname;
    requests.push({ path, body });
    if (body.path === "/work/forbidden.md") return new Response('{"error":"Preview unavailable"}', { status: 404 });
    const data = body.path === "/work/current.ts" ? { data: "const current = 42;", media_type: "text/plain" } : body.paths ? { paths: body.paths, home: "/home/fixture" }
      : body.kind === "image" ? { data: "data:image/png;base64,cGl4ZWw=", media_type: "image/png" }
      : body.kind === "page" ? { data: "<main>REST page</main>", media_type: "text/html" }
      : { data: "# REST document", media_type: "text/markdown" };
    return new Response(JSON.stringify(data), { headers: { "Content-Type": "application/json" } });
  });
  vi.stubGlobal("fetch", fetchMock);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  host.remove();
  vi.unstubAllGlobals();
  configureHtmlFileLoader(null);
  configureLocalArtifactLoader(null);
  configureLocalArtifactOpener(null);
  localStorage.clear();
});

it("renders image, document and page embeds through invoke and authenticated webFetch", async () => {
  await act(async () => root?.render(<LocalArtifactScope tabId="owned-tab">
    <LocalArtifactBlock directive={{ kind: "image", path: "/work/pixel.png" }} renderMarkdown={(text) => text} />
    <LocalArtifactBlock directive={{ kind: "document", path: "/work/report.md" }} renderMarkdown={(text) => <RichMarkdown text={text} />} />
    <LocalArtifactBlock directive={{ kind: "page", path: "/work/view.html" }} renderMarkdown={(text) => text} />
  </LocalArtifactScope>));
  expect(host.querySelector('img[alt="pixel.png"]')?.getAttribute("src")).toBe("data:image/png;base64,cGl4ZWw=");
  expect(host.querySelector("h1")?.textContent).toBe("REST document");
  expect(host.querySelector('iframe[title="HTML preview"]')?.getAttribute("srcdoc")).toContain("REST page");
  expect(requests).toEqual([
    { path: "/api/tabs/owned-tab/preview/read", body: { path: "/work/pixel.png", kind: "image" } },
    { path: "/api/tabs/owned-tab/preview/read", body: { path: "/work/report.md", kind: "document" } },
    { path: "/api/tabs/owned-tab/preview/read", body: { path: "/work/view.html", kind: "page" } },
  ]);
  expect(host.querySelector<HTMLButtonElement>('button[aria-label="Reveal pixel.png"]')?.disabled).toBe(true);
});

it("sends the scoped batch check and refuses a missing tab before network access", async () => {
  expect(await invoke("check_local_preview_files", { paths: ["/work/report.md"], tabId: "owned-tab" })).toEqual({ paths: ["/work/report.md"], home: "/home/fixture" });
  expect(requests).toEqual([{ path: "/api/tabs/owned-tab/preview/check", body: { paths: ["/work/report.md"] } }]);
  await expect(invoke("read_local_preview_file", { path: "/work/report.md", kind: "document" })).rejects.toThrow("tab identity");
  expect(fetchMock).toHaveBeenCalledTimes(1);
});

it("shows REST refusals without rendering unavailable document contents", async () => {
  await act(async () => root?.render(<LocalArtifactScope tabId="owned-tab"><LocalArtifactBlock directive={{ kind: "document", path: "/work/forbidden.md" }} renderMarkdown={(text) => <RichMarkdown text={text} />} /></LocalArtifactScope>));
  expect(host.querySelector('[role="alert"]')?.textContent).toContain("404");
  expect(host.querySelector("h1")).toBeNull();
});

it("loads the side panel document reader through the same scoped REST bridge", async () => {
  await act(async () => root?.render(<DocumentDetail tabId="reader-tab" document={{ path: "/work/report.md", kind: "document", name: "report.md", shownAt: null }} home={null} onBack={() => {}} />));
  expect(host.querySelector("h1")?.textContent).toBe("REST document");
  expect(requests).toEqual([{ path: "/api/tabs/reader-tab/preview/read", body: { path: "/work/report.md", kind: "document" } }]);
  expect(host.querySelector<HTMLButtonElement>('button[aria-label="Reveal report.md"]')?.disabled).toBe(true);
});

it("reads a changed source file through the authenticated tab-scoped web bridge", async () => {
  await act(async () => root?.render(<ChangeFileView tabId="change-owner" target="/work/current.ts" deleted={false} onDiff={() => {}} />));
  expect(host.querySelector("code")?.textContent).toContain("const current = 42;");
  expect(requests).toEqual([{ path: "/api/tabs/change-owner/preview/read", body: { path: "/work/current.ts", kind: "document" } }]);
  expect(fetchMock).toHaveBeenCalledExactlyOnceWith(expect.stringContaining("/api/tabs/change-owner/preview/read"), expect.objectContaining({ method: "POST", body: JSON.stringify({ path: "/work/current.ts", kind: "document" }) }));
});

it("loads a focused chip preview and opens its image via the authenticated attachment reader", async () => {
  await act(async () => root?.render(<LocalArtifactScope tabId="chip-owner"><AttachmentCapsule attachment={{ kind: "image", path: "/work/pixel.png", label: "image1" }} /></LocalArtifactScope>));
  const chip = host.querySelector<HTMLButtonElement>('button[aria-label="Open image image1"]')!;
  expect(chip.querySelector("img")).toBeNull();
  await act(async () => chip.focus());
  expect(document.querySelector('[role="region"] img')?.getAttribute("src")).toBe("data:image/png;base64,cGl4ZWw=");
  await act(async () => chip.click());
  expect(document.querySelector('[role="dialog"] img')?.getAttribute("src")).toBe("data:image/png;base64,cGl4ZWw=");
  expect(requests).toEqual([{ path: "/api/tabs/chip-owner/preview/read", body: { path: "/work/pixel.png", kind: "image" } }]);
});
