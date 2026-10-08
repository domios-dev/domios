import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { configureLocalArtifactLoader, configureLocalArtifactOpener, LocalArtifactBlock, RichMarkdown } from "@codeterm/chat-ui";
import { DocumentsButton, DocumentsPanel } from "./DocumentsPanel";
import { chatDocumentsStore } from "./chatDocumentsStore";
import { planSidePanelStore } from "../Canvas/planSidePanelStore";
import { documentsFromTranscript } from "./chatDocuments";

vi.mock("../../lib/bridge", () => ({ invoke: vi.fn() }));
import { invoke } from "../../lib/bridge";
beforeEach(() => { vi.mocked(invoke).mockImplementation(async (_command, args) => ({ paths: args?.paths ?? [], home: null })); });

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let root: Root | null = null;
let host: HTMLDivElement;
async function render(ui: ReactElement) {
  host = document.createElement("div"); document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(ui));
}
function button(label: string) {
  const result = Array.from(document.querySelectorAll("button")).find((element) => element.getAttribute("aria-label") === label);
  expect(result).toBeTruthy();
  return result!;
}
afterEach(async () => {
  await act(async () => root?.unmount()); root = null; host?.remove();
  planSidePanelStore.close(); planSidePanelStore.resetDocuments(); chatDocumentsStore.clear(); vi.clearAllMocks();
  configureLocalArtifactLoader(null); configureLocalArtifactOpener(null);
});
it("hides the header control for an empty transcript and opens the document slot with the deduplicated count", async () => {
  chatDocumentsStore.set("docs", []);
  await render(<DocumentsButton tabId="docs" />);
  expect(host.querySelector("button")).toBeNull();
  await act(async () => chatDocumentsStore.set("docs", documentsFromTranscript([{ content: "@ds-document /a.md\n@ds-image /b.png\n@ds-document /a.md" }])));
  await act(async () => button("Documents (2)").click());
  expect(planSidePanelStore.get()?.target.documentsTabId).toBe("docs");
});
it("remembers each tab's open state without reopening a dismissed panel", () => {
  planSidePanelStore.open({ planId: "", documentsTabId: "a" });
  planSidePanelStore.activateTab("b"); expect(planSidePanelStore.get()).toBeNull();
  planSidePanelStore.open({ planId: "", documentsTabId: "b" });
  planSidePanelStore.close();
  planSidePanelStore.activateTab("a"); expect(planSidePanelStore.get()?.target.documentsTabId).toBe("a");
  planSidePanelStore.activateTab("b"); expect(planSidePanelStore.get()).toBeNull();
});
it("renders the selected document and lets a list entry open fullscreen or reveal its file", async () => {
  const revealed: string[] = [];
  configureLocalArtifactLoader(async (path) => ({ data: `# ${path}`, media_type: "text/markdown" }));
  configureLocalArtifactOpener(async (path) => { revealed.push(path); });
  chatDocumentsStore.set("reader", documentsFromTranscript([{ content: "@ds-document /old.md\n@ds-document /new.md" }]));
  await render(<DocumentsPanel tabId="reader" panelRef={{ current: null }} state="open" onClose={() => {}} />);
  const nav = host.querySelector("nav")!;
  expect(nav.querySelectorAll('button[role="treeitem"]')).toHaveLength(2);
  await act(async () => Array.from(nav.querySelectorAll("button")).find((item) => item.textContent?.includes("old.md"))?.click());
  expect(host.querySelector("h1")?.textContent).toBe("/old.md");
  await act(async () => button("Fullscreen new.md").click());
  expect(document.querySelector('[role="dialog"][aria-label="Fullscreen new.md"] h1')?.textContent).toBe("/new.md");
  await act(async () => window.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })));
  expect(document.querySelector('[aria-label="Fullscreen new.md"][role="dialog"]')).toBeNull();
  await act(async () => button("Reveal old.md").click());
  expect(revealed).toEqual(["/old.md"]);
});
it("the embedded document fullscreen closes with Escape and restores its opener", async () => {
  configureLocalArtifactLoader(async () => ({ data: "# Readable document", media_type: "text/markdown" }));
  await render(<LocalArtifactBlock directive={{ kind: "document", path: "/report.md" }} renderMarkdown={(text) => <RichMarkdown text={text} />} />);
  const opener = button("Fullscreen report.md"); opener.focus();
  await act(async () => opener.click());
  expect(document.querySelector('[role="dialog"] h1')?.textContent).toBe("Readable document");
  await act(async () => window.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })));
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(document.activeElement).toBe(opener);
});


it("skips missing files in both the badge and tree, and hides the icon after the last file disappears", async () => {
  let paths = ["/work/live.md"];
  vi.mocked(invoke).mockImplementation(async () => ({ paths, home: null }));
  chatDocumentsStore.set("missing", documentsFromTranscript([{ content: "@ds-document /work/gone.md\n@ds-document /work/live.md" }]));
  configureLocalArtifactLoader(async () => ({ data: "# Existing", media_type: "text/markdown" }));
  await render(<><DocumentsButton tabId="missing" /><DocumentsPanel tabId="missing" panelRef={{ current: null }} state="open" onClose={() => {}} /></>);
  expect(button("Documents (1)")).toBeTruthy();
  expect(host.querySelector('[aria-label="Open gone.md"]')).toBeNull();
  expect(host.querySelector('[aria-label="Open live.md"]')).toBeTruthy();
  paths = [];
  await act(async () => { await chatDocumentsStore.refresh("missing"); });
  expect(host.querySelector('[aria-label^="Documents ("]')).toBeNull();
  expect(host.querySelector('[aria-label="Open live.md"]')).toBeNull();
});

it("moves focus through visible rows, opens with Enter, collapses folders and opens fullscreen with Space", async () => {
  configureLocalArtifactLoader(async (path) => ({ data: `# ${path}`, media_type: "text/markdown" }));
  chatDocumentsStore.set("keys", documentsFromTranscript([{ content: "@ds-document /work/a.md\n@ds-document /work/b.md" }]));
  await render(<DocumentsPanel tabId="keys" panelRef={{ current: null }} state="open" onClose={() => {}} />);
  const key = async (code: string) => { await act(async () => document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { code, bubbles: true }))); };
  await act(async () => button("/work").focus());
  await key("ArrowDown");
  expect(document.activeElement).toBe(button("Open a.md"));
  await key("Enter");
  expect(button("Open a.md").getAttribute("aria-selected")).toBe("true");
  expect(host.querySelector("h1")?.textContent).toBe("/work/a.md");
  await key("ArrowDown");
  expect(document.activeElement).toBe(button("Open b.md"));
  await key("Space");
  expect(document.querySelector('[role="dialog"][aria-label="Fullscreen b.md"] h1')?.textContent).toBe("/work/b.md");
  await act(async () => window.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })));
  await act(async () => button("/work").focus());
  await key("ArrowLeft");
  expect(host.querySelectorAll('[role="treeitem"]')).toHaveLength(1);
  await key("ArrowRight");
  expect(host.querySelectorAll('[role="treeitem"]')).toHaveLength(3);
});

it("keeps file actions reachable on keyboard focus and reveals the selected file once", async () => {
  const revealed: string[] = [];
  configureLocalArtifactLoader(async () => ({ data: "# Report", media_type: "text/markdown" }));
  configureLocalArtifactOpener(async (path) => { revealed.push(path); });
  chatDocumentsStore.set("actions", documentsFromTranscript([{ content: "@ds-document /report.md", timestamp: "2026-10-01T10:00:00Z" }]));
  await render(<DocumentsPanel tabId="actions" panelRef={{ current: null }} state="open" onClose={() => {}} />);
  await act(async () => button("Open report.md").focus());
  const reveal = button("Reveal report.md");
  expect(reveal.tabIndex).toBe(0);
  await act(async () => { reveal.focus(); reveal.click(); });
  expect(revealed).toEqual(["/report.md"]);
  expect(button("Open report.md").title).toContain("/report.md\nLast shown:");
});

it("checks once again when an open panel is reopened during its close animation", async () => {
  configureLocalArtifactLoader(async () => ({ data: "# Live", media_type: "text/markdown" }));
  chatDocumentsStore.set("reopen", documentsFromTranscript([{ content: "@ds-document /live.md" }]));
  await chatDocumentsStore.refresh("reopen");
  vi.mocked(invoke).mockClear();
  const panelRef = { current: null };
  const panel = (state: "open" | "closing" | "entering") => <DocumentsPanel tabId="reopen" panelRef={panelRef} state={state} onClose={() => {}} />;
  await render(panel("open"));
  expect(vi.mocked(invoke)).toHaveBeenCalledTimes(1);
  await act(async () => root?.render(panel("closing")));
  expect(vi.mocked(invoke)).toHaveBeenCalledTimes(1);
  await act(async () => root?.render(panel("entering")));
  expect(vi.mocked(invoke)).toHaveBeenCalledTimes(2);
  await act(async () => root?.render(panel("open")));
  expect(vi.mocked(invoke)).toHaveBeenCalledTimes(2);
});
