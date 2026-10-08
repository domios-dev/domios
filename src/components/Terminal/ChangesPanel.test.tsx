import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { ChangesButton } from "./ChangesPanel";
import { chatChangesStore } from "./chatChangesStore";
import { composerInsertionStore } from "./composerInsertion";
import { TabSidePanel } from "./tabPanel/TabSidePanel";
import { tabPanelNavigationStore } from "./tabPanel/tabPanelNavigation";
import { planSidePanelStore } from "../Canvas/planSidePanelStore";
import { clearChatReveal, currentChatReveal } from "../../lib/chatMessageReveal";
import { invoke } from "../../lib/bridge";
import type { NormalizedChatMessage } from "@codeterm/chat-core";

const hostMode = vi.hoisted(() => ({ web: false }));
vi.mock("../../lib/bridge", () => ({ get IS_WEB() { return hostMode.web; }, invoke: vi.fn(() => Promise.resolve()) }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let root: Root | null = null;
let host: HTMLDivElement;
async function render(ui: ReactElement) {
  host = document.createElement("div"); document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(ui));
}
function button(name: string) {
  const found = Array.from(host.querySelectorAll("button")).find((item) => (item.getAttribute("aria-label") ?? item.textContent) === name);
  expect(found, name).toBeTruthy();
  return found!;
}
const tab = (value: string) => host.querySelector<HTMLButtonElement>(`[role="tablist"][aria-label="Changes scope"] [data-value="${value}"]`)!;
const turnSelect = () => host.querySelector<HTMLSelectElement>('select[aria-label="Turn"]')!;
async function chooseTurn(turnId: string) {
  await act(async () => { const select = turnSelect(); select.value = turnId; select.dispatchEvent(new Event("change", { bubbles: true })); });
}
const messages: NormalizedChatMessage[] = [
  { id: "turn1", sourceMessageId: "source1", type: "user", content: "work" },
  { id: "edit1", type: "tool_result", content: "", toolEdits: [{ path: "src/a.ts", old: "x", new: "y" }] },
  { id: "turn2", type: "user", content: "next" },
  { id: "edit2", type: "tool_result", content: "", toolEdits: [{ path: "src/b.ts", old: "", new: "new" }] },
];
const panel = (onClose = () => {}) => <TabSidePanel tabId="tab" view="changes" cwd="/repo" panelRef={{ current: null }} state="open" onClose={onClose} />;
afterEach(async () => {
  await act(async () => root?.unmount()); root = null; host?.remove();
  hostMode.web = false;
  chatChangesStore.clear(); planSidePanelStore.close(); planSidePanelStore.resetDocuments(); tabPanelNavigationStore.clear(); composerInsertionStore.clear(); clearChatReveal(); vi.clearAllMocks();
});

it("hides an empty tab's icon and opens its own changes with compact recorded totals", async () => {
  await render(<ChangesButton tabId="tab" />);
  expect(host.querySelector("button")).toBeNull();
  await act(async () => chatChangesStore.set("another", messages));
  expect(host.querySelector("button")).toBeNull();
  await act(async () => chatChangesStore.set("tab", messages));
  await act(async () => button("Changes (+2 -1)").click());
  expect(planSidePanelStore.get()?.target).toEqual({ planId: "", documentsTabId: "tab", tabView: "changes" });
  expect(button("Changes (+2 -1)").getAttribute("aria-expanded")).toBe("true");
  const big = Array.from({ length: 1234 }, (_, index) => `line ${index}`).join("\n");
  await act(async () => chatChangesStore.set("tab", [{ id: "big", type: "tool_result", content: "", toolEdits: [{ path: "big.ts", old: "", new: big }] }]));
  expect(button("Changes (+1234 -0)").textContent).toContain("+1.2k");
  expect(button("Changes (+1234 -0)").title).toContain("1,234 added");
});

it("shows status glyphs in the tree and scopes it to a turn with the picker", async () => {
  chatChangesStore.set("tab", [...messages, { id: "edit3", type: "tool_result", content: "", fileDiffs: [{ path: "src/gone.ts", kind: "delete", additions: 0, deletions: 1, hunks: [{ oldStart: 1, oldLines: 1, newStart: 0, newLines: 0, lines: ["-gone"] }] }] }]);
  await render(panel());
  expect(tab("all").getAttribute("aria-selected")).toBe("true");
  expect(button("Show changes for src/a.ts").textContent).toContain("Modified");
  expect(button("Show changes for src/gone.ts").textContent).toContain("Deleted");
  await act(async () => tab("turn").click());
  expect(turnSelect().value).toBe("turn2");
  expect(host.querySelector('[aria-label="Show changes for src/a.ts"]')).toBeNull();
  await act(async () => button("Older turn").click());
  expect(turnSelect().value).toBe("source1");
  expect(host.querySelector('[aria-label="Show changes for src/b.ts"]')).toBeNull();
  await act(async () => button("Newer turn").click());
  expect(turnSelect().value).toBe("turn2");
});

it("opens a file's diff, jumps to its turn, runs file actions and goes back with Escape", async () => {
  const onClose = vi.fn();
  chatChangesStore.set("tab", messages);
  await render(panel(onClose));
  await act(async () => tab("turn").click());
  await chooseTurn("source1");
  await act(async () => button("Show changes for src/a.ts").click());
  expect(host.querySelector('[role="region"][aria-label="Diff for src/a.ts"]')?.hasAttribute("hidden")).toBe(false);
  expect(host.querySelector('nav[aria-label="Path"]')?.textContent).toContain("a.ts");
  await act(async () => button("Jump to turn").click());
  expect(currentChatReveal()).toMatchObject({ tabId: "tab", messageId: "source1", outcome: "pending" });
  expect(button("Jump to turn").textContent).toBe("Turn 1");
  await act(async () => button("Open file").click());
  expect(invoke).toHaveBeenCalledWith("open_folder", { path: "/repo/src/a.ts" });
  await act(async () => button("Show in Explorer/Finder").click());
  expect(invoke).toHaveBeenCalledWith("reveal_file", { path: "/repo/src/a.ts" });
  await act(async () => button("Back to list").dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true, cancelable: true })));
  expect(onClose).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(button("Show changes for src/a.ts"));
  expect(turnSelect().value).toBe("source1");
});

it("adds a change and a hunk to the composer as diff capsules and copies the diff without sending", async () => {
  const writeText = vi.fn(async () => {});
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
  chatChangesStore.set("tab", messages);
  await render(panel());
  await act(async () => button("Show changes for src/a.ts").click());
  await act(async () => button("Ask about the change to src/a.ts").click());
  expect(composerInsertionStore.take("tab")).toEqual([{
    kind: "diff",
    label: "diff · a.ts +1 −1",
    content: "Recorded change in `src/a.ts` (modified, +1 -1; @@ -1,1 +1,1 @@)\n\nCurrent file: `/repo/src/a.ts`\n\n```diff\n--- a/src/a.ts\n+++ b/src/a.ts\n@@ -1,1 +1,1 @@\n-x\n+y\n```",
  }]);
  await act(async () => button("Add hunk @@ -1,1 +1,1 @@ to chat").click());
  expect(composerInsertionStore.take("tab")).toEqual([{ kind: "diff", label: "diff · a.ts +1 −1", content: "Recorded change in `src/a.ts:1`:\n```diff\n@@ -1,1 +1,1 @@\n-x\n+y\n```" }]);
  await act(async () => button("Copy diff").click());
  expect(writeText).toHaveBeenCalledWith("--- a/src/a.ts\n+++ b/src/a.ts\n@@ -1,1 +1,1 @@\n-x\n+y");
  expect(invoke).not.toHaveBeenCalledWith(expect.stringMatching(/send/), expect.anything());
});

it("keeps changes remembered per tab and switches to documents in the same host slot", async () => {
  planSidePanelStore.open({ planId: "", documentsTabId: "tab", tabView: "changes" });
  planSidePanelStore.activateTab("other");
  expect(planSidePanelStore.get()).toBeNull();
  planSidePanelStore.activateTab("tab");
  expect(planSidePanelStore.get()?.target.tabView).toBe("changes");
  await render(panel());
  await act(async () => host.querySelector<HTMLButtonElement>('[role="tablist"][aria-label="Side panel view"] [data-value="documents"]')!.click());
  expect(planSidePanelStore.get()?.target).toEqual({ planId: "", documentsTabId: "tab", tabView: "documents" });
});

it("keeps failed file actions visible and disables opening deleted files", async () => {
  chatChangesStore.set("tab", messages);
  await render(panel());
  await act(async () => button("Show changes for src/a.ts").click());
  vi.mocked(invoke).mockRejectedValueOnce(new Error("File is unavailable"));
  await act(async () => button("Open file").click());
  expect(host.querySelector('[role="alert"]')).toBeTruthy();
  await act(async () => chatChangesStore.set("tab", [{ id: "deleted", type: "tool_result", content: "", fileDiffs: [{ path: "gone.ts", kind: "delete", additions: 0, deletions: 0, hunks: [] }] }]));
  await act(async () => button("Show changes for gone.ts").click());
  expect(button("Open file").disabled).toBe(true);
  expect(button("Show in Explorer/Finder").disabled).toBe(true);
  await act(async () => button("Ask about the change to gone.ts").click());
  expect(composerInsertionStore.take("tab")[0]?.content).not.toContain("Current file");
  button("Jump to turn");
});

it("opens a historical file at its latest recorded rename destination", async () => {
  chatChangesStore.set("tab", [...messages,
    { id: "move", type: "tool_result", content: "", fileDiffs: [{ path: "src/a.ts", movePath: "src/moved.ts", additions: 0, deletions: 0, hunks: [] }] },
  ]);
  await render(panel());
  await act(async () => tab("turn").click());
  await chooseTurn("source1");
  await act(async () => button("Show changes for src/a.ts").click());
  await act(async () => button("Open file").click());
  expect(invoke).toHaveBeenCalledWith("open_folder", { path: "/repo/src/moved.ts" });
});

it("invalidates the cached summary when the tab adopts another session", () => {
  chatChangesStore.bind("tab", "session1");
  chatChangesStore.set("tab", messages);
  chatChangesStore.bind("tab", "session1");
  expect(chatChangesStore.get("tab").cumulative).toHaveLength(2);
  chatChangesStore.bind("tab", "session2");
  expect(chatChangesStore.get("tab").cumulative).toHaveLength(0);
});

it("keeps binary edits actionable without rendering an empty text diff", async () => {
  chatChangesStore.set("tab", [{ id: "binary", type: "tool_result", content: "", fileDiffs: [{ path: "image.png", binary: true, additions: 0, deletions: 0, hunks: [] }] }]);
  await render(panel());
  await act(async () => button("Show changes for image.png").click());
  expect(host.querySelector('[role="region"][aria-label="Diff for image.png"]')).toBeNull();
  expect(button("Open file").disabled).toBe(false);
  expect(button("Copy diff").disabled).toBe(true);
  button("Jump to turn");
});

it("shows one helpful line when the tab has no recorded edits", async () => {
  await render(panel());
  expect(host.textContent).toContain("File edits the agent makes in this tab appear here.");
  expect(host.querySelector('[role="tree"]')).toBeNull();
});


it("disables host OS actions in the web reader without invoking them", async () => {
  hostMode.web = true;
  chatChangesStore.set("tab", messages);
  await render(panel());
  await act(async () => button("Show changes for src/a.ts").click());
  expect(button("Open file").disabled).toBe(true);
  expect(button("Show in Explorer/Finder").disabled).toBe(true);
  await act(async () => { button("Open file").click(); button("Show in Explorer/Finder").click(); });
  expect(invoke).not.toHaveBeenCalledWith("open_folder", expect.anything());
  expect(invoke).not.toHaveBeenCalledWith("reveal_file", expect.anything());
});
