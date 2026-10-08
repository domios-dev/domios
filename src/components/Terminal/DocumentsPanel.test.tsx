import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it } from "vitest";
import { configureLocalArtifactLoader, configureLocalArtifactOpener, LocalArtifactBlock, RichMarkdown } from "@codeterm/chat-ui";
import { DocumentsButton, DocumentsPanel } from "./DocumentsPanel";
import { chatDocumentsStore } from "./chatDocumentsStore";
import { planSidePanelStore } from "../Canvas/planSidePanelStore";
import { documentsFromTranscript } from "./chatDocuments";

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
  planSidePanelStore.close(); planSidePanelStore.resetDocuments();
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
  const nav = host.querySelector('nav[aria-label="Tab documents"]')!;
  expect(nav.querySelector("button")?.textContent).toContain("new.md");
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
