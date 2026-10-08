import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { ChangesButton, ChangesPanel } from "./ChangesPanel";
import { chatChangesStore } from "./chatChangesStore";
import { planSidePanelStore } from "../Canvas/planSidePanelStore";
import { clearChatReveal, currentChatReveal } from "../../lib/chatMessageReveal";
import { invoke } from "../../lib/bridge";
import type { NormalizedChatMessage } from "@codeterm/chat-core";

vi.mock("../../lib/bridge", () => ({ invoke: vi.fn(() => Promise.resolve()) }));
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
  expect(found).toBeTruthy();
  return found!;
}
const messages: NormalizedChatMessage[] = [
  { id: "turn1", sourceMessageId: "source1", type: "user", content: "work" },
  { id: "edit1", type: "tool_result", content: "", toolEdits: [{ path: "src/a.ts", old: "x", new: "y" }] },
  { id: "turn2", type: "user", content: "next" },
  { id: "edit2", type: "tool_result", content: "", toolEdits: [{ path: "src/b.ts", old: "", new: "new" }] },
];
const panel = () => <ChangesPanel tabId="tab" cwd="/repo" panelRef={{ current: null }} state="open" onClose={() => {}} />;
afterEach(async () => {
  await act(async () => root?.unmount()); root = null; host?.remove();
  chatChangesStore.clear(); planSidePanelStore.close(); planSidePanelStore.resetDocuments(); clearChatReveal(); vi.clearAllMocks();
});

it("hides an empty tab's icon and opens its own changes with recorded totals", async () => {
  await render(<ChangesButton tabId="tab" />);
  expect(host.querySelector("button")).toBeNull();
  await act(async () => chatChangesStore.set("another", messages));
  expect(host.querySelector("button")).toBeNull();
  await act(async () => chatChangesStore.set("tab", messages));
  await act(async () => button("Changes (+2 -1)").click());
  expect(planSidePanelStore.get()?.target).toEqual({ planId: "", documentsTabId: "tab", tabView: "changes" });
  expect(button("Changes (+2 -1)").getAttribute("aria-expanded")).toBe("true");
});

it("switches cumulative to latest turn, selects a file, opens its diffs and jumps by source turn identity", async () => {
  chatChangesStore.set("tab", messages);
  await render(panel());
  expect(button("Cumulative").getAttribute("aria-pressed")).toBe("true");
  button("Show changes for src/a.ts"); button("Show changes for src/b.ts");
  await act(async () => button("Per turn").click());
  const select = host.querySelector<HTMLSelectElement>('select[aria-label="Turn"]')!;
  expect(select.value).toBe("turn2");
  expect(host.querySelector('[aria-label="Show changes for src/a.ts"]')).toBeNull();
  await act(async () => button("Show changes for src/b.ts").click());
  expect(host.querySelector('[role="region"][aria-label="Diff for src/b.ts"]')?.hasAttribute("hidden")).toBe(false);
  await act(async () => { select.value = "source1"; select.dispatchEvent(new Event("change", { bubbles: true })); });
  await act(async () => button("Show changes for src/a.ts").click());
  await act(async () => button("Jump to turn").click());
  expect(currentChatReveal()).toMatchObject({ tabId: "tab", messageId: "source1", outcome: "pending" });
  await act(async () => button("Open file").click());
  expect(invoke).toHaveBeenCalledWith("open_folder", { path: "/repo/src/a.ts" });
  await act(async () => button("Show in Explorer/Finder").click());
  expect(invoke).toHaveBeenCalledWith("reveal_file", { path: "/repo/src/a.ts" });
});

it("keeps changes remembered per tab and switches to documents in the same host slot", async () => {
  planSidePanelStore.open({ planId: "", documentsTabId: "tab", tabView: "changes" });
  planSidePanelStore.activateTab("other");
  expect(planSidePanelStore.get()).toBeNull();
  planSidePanelStore.activateTab("tab");
  expect(planSidePanelStore.get()?.target.tabView).toBe("changes");
  await render(panel());
  await act(async () => button("Documents").click());
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
  button("Jump to turn");
});

it("opens a historical file at its latest recorded rename destination", async () => {
  chatChangesStore.set("tab", [...messages,
    { id: "move", type: "tool_result", content: "", fileDiffs: [{ path: "src/a.ts", movePath: "src/moved.ts", additions: 0, deletions: 0, hunks: [] }] },
  ]);
  await render(panel());
  await act(async () => button("Per turn").click());
  const select = host.querySelector<HTMLSelectElement>('select[aria-label="Turn"]')!;
  await act(async () => { select.value = "source1"; select.dispatchEvent(new Event("change", { bubbles: true })); });
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
  button("Jump to turn");
});
