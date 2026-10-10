import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { configureHtmlFileLoader, configureLocalArtifactLoader, configureLocalArtifactOpener, LocalArtifactScope, LocalArtifactBlock, RichMarkdown } from "@codeterm/chat-ui";
import { DocumentsButton } from "./DocumentsPanel";
import { chatDocumentsStore } from "./chatDocumentsStore";
import { planSidePanelStore } from "../Canvas/planSidePanelStore";
import { documentsFromTranscript } from "./chatDocuments";
import { composerInsertionStore } from "./composerInsertion";
import { TabSidePanel } from "./tabPanel/TabSidePanel";
import { tabPanelNavigationStore } from "./tabPanel/tabPanelNavigation";
import { themeToCSS, DEFAULT_THEME } from "../../lib/themeCss";

vi.mock("../../lib/bridge", () => ({ invoke: vi.fn() }));
import { invoke } from "../../lib/bridge";
beforeEach(() => { vi.mocked(invoke).mockImplementation(async (_command, args) => ({ paths: (args as { paths?: string[] })?.paths ?? [], home: null })); });

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
  expect(result, label).toBeTruthy();
  return result!;
}
const key = async (code: string, target: Element | null = document.activeElement) => {
  await act(async () => target?.dispatchEvent(new KeyboardEvent("keydown", { code, bubbles: true, cancelable: true })));
};
const panel = (tabId: string, onClose = () => {}, state: "open" | "closing" | "entering" = "open") =>
  <TabSidePanel tabId={tabId} view="documents" panelRef={{ current: null }} state={state} onClose={onClose} />;
afterEach(async () => {
  await act(async () => root?.unmount()); root = null; host?.remove();
  planSidePanelStore.close(); planSidePanelStore.resetDocuments(); chatDocumentsStore.clear(); tabPanelNavigationStore.clear(); composerInsertionStore.clear(); vi.clearAllMocks();
  configureHtmlFileLoader(null); configureLocalArtifactLoader(null); configureLocalArtifactOpener(null);
  document.documentElement.removeAttribute("style");
});

it("hides the header control for an empty transcript and opens the panel with a deduplicated count", async () => {
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

it("opens a document from the tree into a reader, and Back, Escape or Backspace return to the focused row", async () => {
  const onClose = vi.fn();
  configureLocalArtifactLoader(async (path) => ({ data: `# ${path}`, media_type: "text/markdown" }));
  chatDocumentsStore.set("reader", documentsFromTranscript([{ content: "@ds-document /work/old.md\n@ds-document /work/new.md" }]));
  await render(panel("reader", onClose));
  expect(host.querySelectorAll('[role="tree"] [role="treeitem"]')).toHaveLength(3);
  await act(async () => button("Open old.md").click());
  expect(host.querySelector('[role="tree"]')).toBeNull();
  expect(host.querySelector("h1")?.textContent).toBe("/work/old.md");
  expect(host.querySelector('nav[aria-label="Path"]')?.textContent).toContain("old.md");
  await act(async () => button("Back to list").click());
  expect(document.activeElement).toBe(button("Open old.md"));
  await act(async () => button("Open new.md").click());
  await key("Escape", host.querySelector("h1"));
  expect(onClose).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(button("Open new.md"));
  await act(async () => button("Open new.md").click());
  await key("Backspace", button("Back to list"));
  expect(host.querySelector('[role="tree"]')).toBeTruthy();
  await key("Escape", button("Open new.md"));
  expect(onClose).toHaveBeenCalledTimes(1);
});

it("keeps the open document per tab across remounts", async () => {
  configureLocalArtifactLoader(async () => ({ data: "# Kept", media_type: "text/markdown" }));
  chatDocumentsStore.set("kept", documentsFromTranscript([{ content: "@ds-document /kept.md" }]));
  await render(panel("kept"));
  await act(async () => button("Open kept.md").click());
  await act(async () => root?.unmount());
  root = createRoot(host);
  await act(async () => root?.render(panel("kept")));
  expect(host.querySelector("h1")?.textContent).toBe("Kept");
});

it("moves focus through visible rows, opens with Enter and collapses folders with the arrows", async () => {
  configureLocalArtifactLoader(async (path) => ({ data: `# ${path}`, media_type: "text/markdown" }));
  chatDocumentsStore.set("keys", documentsFromTranscript([{ content: "@ds-document /work/a.md\n@ds-document /work/b.md" }]));
  await render(panel("keys"));
  await act(async () => button("/work").focus());
  await key("ArrowDown");
  expect(document.activeElement).toBe(button("Open a.md"));
  await key("End");
  expect(document.activeElement).toBe(button("Open b.md"));
  await key("ArrowLeft");
  expect(document.activeElement).toBe(button("/work"));
  await key("ArrowLeft");
  expect(host.querySelectorAll('[role="treeitem"]')).toHaveLength(1);
  await key("ArrowRight");
  expect(host.querySelectorAll('[role="treeitem"]')).toHaveLength(3);
  await key("ArrowDown");
  await key("Enter");
  expect(host.querySelector("h1")?.textContent).toBe("/work/a.md");
});

it("reader actions open fullscreen, reveal, copy the path and content, and add the document to chat without sending", async () => {
  const revealed: string[] = [];
  const writeText = vi.fn(async () => {});
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
  configureLocalArtifactLoader(async () => ({ data: "# Report\n\nBody line", media_type: "text/markdown" }));
  configureLocalArtifactOpener(async (path) => { revealed.push(path); });
  chatDocumentsStore.set("actions", documentsFromTranscript([{ content: "@ds-document /report.md" }]));
  await render(panel("actions"));
  await act(async () => button("Open report.md").click());
  await act(async () => button("Fullscreen report.md").click());
  expect(document.querySelector('[role="dialog"][aria-label="Fullscreen report.md"] h1')?.textContent).toBe("Report");
  await act(async () => window.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })));
  await act(async () => button("Reveal report.md").click());
  expect(revealed).toEqual(["/report.md"]);
  await act(async () => button("Copy path").click());
  await act(async () => button("Copy content").click());
  expect(writeText.mock.calls).toEqual([["/report.md"], ["# Report\n\nBody line"]]);
  await act(async () => button("Ask about report.md").click());
  expect(composerInsertionStore.take("actions")).toEqual([{ kind: "document", path: "/report.md" }]);
});

it("selected reader text offers Copy and Add to chat with a file line reference", async () => {
  configureLocalArtifactLoader(async () => ({ data: "# Title\n\nFirst paragraph here.", media_type: "text/markdown" }));
  chatDocumentsStore.set("select", documentsFromTranscript([{ content: "@ds-document /notes.md" }]));
  await render(panel("select"));
  await act(async () => button("Open notes.md").click());
  const paragraph = Array.from(host.querySelectorAll("p")).find((item) => item.textContent === "First paragraph here.")!;
  await act(async () => {
    const range = document.createRange();
    range.selectNodeContents(paragraph);
    document.getSelection()?.removeAllRanges();
    document.getSelection()?.addRange(range);
    document.dispatchEvent(new Event("selectionchange"));
  });
  const toolbar = host.querySelector('[role="toolbar"][aria-label="Selected text"]')!;
  expect(toolbar).toBeTruthy();
  await act(async () => Array.from(toolbar.querySelectorAll("button")).find((item) => item.textContent === "Add to chat")?.click());
  expect(composerInsertionStore.take("select")).toEqual([{ kind: "quote", detail: "notes.md:3", content: "From `/notes.md:3`:\n> First paragraph here." }]);
});

it("skips missing files and checks again when the panel is reopened during its close animation", async () => {
  let paths = ["/work/live.md"];
  vi.mocked(invoke).mockImplementation(async () => ({ paths, home: null }));
  configureLocalArtifactLoader(async () => ({ data: "# Existing", media_type: "text/markdown" }));
  chatDocumentsStore.set("missing", documentsFromTranscript([{ content: "@ds-document /work/gone.md\n@ds-document /work/live.md" }]));
  await render(<><DocumentsButton tabId="missing" />{panel("missing")}</>);
  expect(button("Documents (1)")).toBeTruthy();
  expect(host.querySelector('[aria-label="Open gone.md"]')).toBeNull();
  const calls = vi.mocked(invoke).mock.calls.length;
  await act(async () => root?.render(<><DocumentsButton tabId="missing" />{panel("missing", () => {}, "closing")}</>));
  expect(vi.mocked(invoke).mock.calls.length).toBe(calls);
  paths = [];
  await act(async () => root?.render(<><DocumentsButton tabId="missing" />{panel("missing", () => {}, "entering")}</>));
  expect(vi.mocked(invoke).mock.calls.length).toBe(calls + 1);
  expect(host.querySelector('[aria-label^="Documents ("]')).toBeNull();
  expect(host.textContent).toContain("Documents the agent shows in this tab appear here.");
});

it("switches views from the segmented control in the shared slot", async () => {
  vi.mocked(invoke).mockResolvedValue({ paths: ["/live.md"], home: null });
  chatDocumentsStore.set("shared", documentsFromTranscript([{ content: "@ds-document /live.md" }]));
  planSidePanelStore.open({ planId: "", documentsTabId: "shared" });
  await render(<><DocumentsButton tabId="shared" />{panel("shared")}</>);
  expect(button("Documents (1)").getAttribute("aria-expanded")).toBe("true");
  const tabs = host.querySelector('[role="tablist"][aria-label="Side panel view"]')!;
  const changes = tabs.querySelector<HTMLButtonElement>('[role="tab"][data-value="changes"]')!;
  expect(tabs.querySelector('[role="tab"][data-value="documents"]')?.getAttribute("aria-selected")).toBe("true");
  await act(async () => changes.click());
  expect(planSidePanelStore.get()?.target).toEqual({ planId: "", documentsTabId: "shared", tabView: "changes" });
  expect(button("Documents (1)").getAttribute("aria-expanded")).toBe("false");
});

it("renders both views under light and dark themes without errors", async () => {
  const errors = vi.spyOn(console, "error").mockImplementation(() => {});
  configureLocalArtifactLoader(async () => ({ data: "# Themed", media_type: "text/markdown" }));
  chatDocumentsStore.set("themed", documentsFromTranscript([{ content: "@ds-document /themed.md" }]));
  const light = { ...DEFAULT_THEME, name: "light", background: "#ffffff", foreground: "#24292f", black: "#24292f", bright_black: "#6e7781", white: "#ffffff", bright_white: "#ffffff" };
  for (const theme of [DEFAULT_THEME, light]) {
    for (const [name, value] of Object.entries(themeToCSS(theme))) document.documentElement.style.setProperty(name, value);
    for (const view of ["documents", "changes"] as const) {
      await render(<TabSidePanel tabId="themed" view={view} panelRef={{ current: null }} state="open" onClose={() => {}} />);
      expect(host.querySelector(`[data-testid="tab-side-panel"][data-view="${view}"]`)).toBeTruthy();
      await act(async () => root?.unmount()); root = null; host.remove();
    }
  }
  expect(errors).not.toHaveBeenCalled();
  errors.mockRestore();
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


it("carries the reader's tab identity to its loader and disables unavailable OS reveal", async () => {
  const load = vi.fn(async () => ({ data: "# Tab owned document", media_type: "text/markdown" }));
  configureLocalArtifactLoader(load);
  configureLocalArtifactOpener(null);
  chatDocumentsStore.set("owner", documentsFromTranscript([{ content: "@ds-document /work/owned.md" }]));
  await render(panel("owner"));
  await act(async () => button("Open owned.md").click());
  expect(load).toHaveBeenCalledWith("/work/owned.md", "document", "owner");
  expect(host.querySelector("h1")?.textContent).toBe("Tab owned document");
  expect(button("Reveal owned.md").disabled).toBe(true);
  expect(invoke).toHaveBeenCalledWith("check_local_preview_files", { paths: ["/work/owned.md"], tabId: "owner" });
});

it("loads inline images with the nearest tab scope and leaves reveal disabled without an OS opener", async () => {
  const load = vi.fn(async () => ({ data: "data:image/png;base64,aW1hZ2U=", media_type: "image/png" }));
  configureLocalArtifactLoader(load);
  configureLocalArtifactOpener(null);
  await render(<LocalArtifactScope tabId="image-owner"><LocalArtifactBlock directive={{ kind: "image", path: "/work/pixel.png" }} renderMarkdown={(text) => text} /></LocalArtifactScope>);
  expect(load).toHaveBeenCalledWith("/work/pixel.png", "image", "image-owner");
  expect(host.querySelector('img[alt="pixel.png"]')?.getAttribute("src")).toBe("data:image/png;base64,aW1hZ2U=");
  expect(button("Reveal pixel.png").disabled).toBe(true);
});


it("loads file-backed HTML using the reader's tab scope", async () => {
  const load = vi.fn(async () => "<main>Owned HTML</main>");
  configureHtmlFileLoader(load);
  await render(<LocalArtifactScope tabId="page-owner"><LocalArtifactBlock directive={{ kind: "page", path: "/work/view.html" }} renderMarkdown={(text) => text} /></LocalArtifactScope>);
  expect(load).toHaveBeenCalledWith("/work/view.html", "page-owner");
  const preview = host.querySelector('iframe[title="HTML preview"]');
  expect(preview?.getAttribute("srcdoc")).toContain("Owned HTML");
});
