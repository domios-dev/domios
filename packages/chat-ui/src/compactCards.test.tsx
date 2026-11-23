import React, { act, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { FileDiff, ToolEvent } from "@codeterm/chat-core";
import { renderChatCardMenu } from "../../../src/components/Terminal/ChatCardMenu";
import { ChatBlockHostProvider } from "./blocks/ChatBlockHost";
import { ChatBlockFrame } from "./blocks/ChatBlockFrame";
import { toolCommandShape } from "./codeterm/commandShape";
import { CommandGroupCard } from "./toolcard/CommandGroupCard";
import { EditRunCard } from "./toolcard/EditRunCard";
import { SearchCard } from "./toolcard/SearchCard";
import { ThoughtLine } from "./toolcard/ThoughtLine";
import { CompactToolRowsProvider } from "./useCompactToolRow";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement;

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  host?.remove();
  vi.restoreAllMocks();
});

function Surface({ children }: { children: React.ReactNode }) {
  const surface = useRef<HTMLDivElement>(null);
  return <CompactToolRowsProvider surfaceRef={surface}><div ref={surface}>{children}</div></CompactToolRowsProvider>;
}

/** Mounts on a phone (coarse pointer) or a desktop (fine pointer, wide column). */
async function mount(phone: boolean, content: React.ReactNode) {
  const matchMedia = window.matchMedia.bind(window);
  vi.spyOn(window, "matchMedia").mockImplementation((query) => {
    const media = matchMedia(query);
    if (query === "(pointer: coarse)") Object.defineProperty(media, "matches", { value: phone });
    return media;
  });
  const bounds = HTMLElement.prototype.getBoundingClientRect;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    return { ...bounds.call(this), width: phone ? 390 : 900 };
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<Surface><ChatBlockHostProvider value={{ renderMenu: renderChatCardMenu, askAbout: () => {} }}>{content}</ChatBlockHostProvider></Surface>));
}

const labelled = (label: string) => document.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
const readerOpeners = () => host.querySelectorAll('button[aria-label^="Open full screen:"]');

async function menu(label: string): Promise<string[]> {
  const kebab = labelled(label);
  expect(kebab).not.toBeNull();
  await act(async () => kebab!.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, cancelable: true })));
  return [...document.querySelectorAll('[role^="menuitem"]')].map((item) => item.textContent?.trim() ?? "");
}

async function choose(label: string) {
  const item = [...document.querySelectorAll<HTMLElement>('[role^="menuitem"]')].find((node) => node.textContent?.trim() === label);
  expect(item).toBeTruthy();
  await act(async () => item!.click());
}

const dialog = (label: string) => document.querySelector(`[role="dialog"][aria-label="${label}"]`);

const CHAIN = "git status ; git log -1 ; ls";
const group: ToolEvent = {
  id: "group", phase: "result", status: "success", kind: "command", summary: CHAIN, detail: CHAIN, description: "Inspect the repo",
  result: "ok", exitCode: 0, durationMs: 1830,
  message: { id: "group", type: "tool_call", content: "", toolName: "Bash", toolInput: { command: CHAIN } },
};
const rows = () => {
  const shape = toolCommandShape(group);
  if (shape.kind !== "group") throw new Error("fixture must be a command group");
  return shape.rows;
};
const search: ToolEvent = {
  id: "search", phase: "result", status: "success", kind: "search", summary: "search 10 matches in 2 files", result: "a\nb",
  message: { id: "search", type: "tool_call", content: "", toolName: "Grep", toolInput: { pattern: "/model" } },
};

describe("chat boxes on a phone", () => {
  it("Thinking puts full screen under its ⋮ instead of a separate icon", async () => {
    await mount(true, <ThoughtLine text={"A long thought.\n".repeat(12)} />);
    expect(readerOpeners()).toHaveLength(0);
    expect(await menu("Actions for Thinking")).toEqual(["Open full screen"]);
    await choose("Open full screen");
    expect(dialog("Thinking")?.textContent).toContain("A long thought.");
  });

  it("a command group shows title, count and ⋮; full screen and Details live in the menu, timing in the reader", async () => {
    let toggles = 0;
    await mount(true, <CommandGroupCard event={group} rows={rows()} expanded={false} onToggle={() => { toggles += 1; }} />);
    expect(readerOpeners()).toHaveLength(0);
    expect(labelled("Expand command group")).toBeNull();
    expect(host.querySelector('[data-testid="toolcard-duration"]')).toBeNull();
    await act(async () => labelled("Show commands in Inspect the repo")!.click());
    expect(toggles).toBe(1);
    expect(await menu("Actions for Inspect the repo")).toEqual(["Open full screen", "Details"]);
    await choose("Open full screen");
    const reader = dialog("Inspect the repo");
    expect(reader?.querySelector('[data-testid="toolcard-duration"]')?.textContent).toBe("1.8s");
    expect(await menu("Actions for Inspect the repo")).toEqual(["Details"]);
  });

  it("a search row keeps one key fact; the reader shows the full counts", async () => {
    await mount(true, <SearchCard event={search} expanded={false} onToggle={() => {}} />);
    expect(host.querySelector('[data-testid="toolcard-search-counts"]')?.textContent).toBe("10 matches");
    expect(await menu("Actions for Search")).toEqual(["Open full screen", "Copy output", "Ask about this", "Details"]);
    await choose("Open full screen");
    expect(dialog("Search")?.querySelector('[data-testid="toolcard-search-counts"]')?.textContent).toBe("10 matches · 2 files");
  });

  it("a block card adds full screen in front of its own actions", async () => {
    await mount(true, <ChatBlockFrame name="opaque" source="opaque-source" title="Opaque block" icon={null} body={<p>Body</p>} />);
    expect(readerOpeners()).toHaveLength(0);
    expect(await menu("Actions for Opaque block")).toEqual(["Open full screen", "Ask about this"]);
  });
});

describe("chat boxes on desktop are unchanged", () => {
  it("keeps the full-screen icon, the chevron and the full facts beside an unchanged menu", async () => {
    await mount(false, <>
      <ThoughtLine text={"A long thought.\n".repeat(12)} />
      <CommandGroupCard event={group} rows={rows()} expanded={false} onToggle={() => {}} />
      <SearchCard event={search} expanded={false} onToggle={() => {}} />
    </>);
    expect(labelled("Open full screen: Thinking")).not.toBeNull();
    expect(labelled("Actions for Thinking")).toBeNull();
    expect(labelled("Open full screen: Inspect the repo")).not.toBeNull();
    expect(labelled("Expand command group")).not.toBeNull();
    expect(host.querySelector('[data-testid="toolcard-duration"]')?.textContent).toBe("1.8s");
    expect(host.querySelector('[data-testid="toolcard-search-counts"]')?.textContent).toBe("10 matches · 2 files");
    expect(await menu("Actions for Inspect the repo")).toEqual(["Open full screen", "Details"]);
  });
});

function modifyDiff(path: string, before: string, after: string): FileDiff {
  return {
    path, kind: "modify", additions: 1, deletions: 1,
    hunks: [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: [`-${before}`, `+${after}`] }],
  };
}

function editMember(id: string, fileDiffs: FileDiff[], overrides: Partial<ToolEvent> = {}): ToolEvent {
  return {
    id, toolId: id, phase: "call", status: "success", kind: "edit", summary: fileDiffs[0]?.path ?? "", fileDiffs,
    message: { id, type: "tool_call", content: "", toolId: id, toolKind: "edit" },
    ...overrides,
  };
}

describe("grouped edit run cards", () => {
  it("keeps distinct operations sharing a path while reporting the distinct file count in the heading", async () => {
    const shared = "src/module_shared.ts";
    const members: ToolEvent[] = [
      editMember("op-1", [modifyDiff(shared, "v1", "v2")], { result: "first edit ok" }),
      editMember("op-2", [modifyDiff(shared, "v2", "v3")], { result: "second edit ok" }),
      editMember("op-3", [modifyDiff("src/module_b.ts", "b1", "b2")], { result: "third edit ok" }),
      editMember("op-4", [modifyDiff("src/module_c.ts", "c1", "c2")], { result: "fourth edit ok" }),
      editMember("op-5", [modifyDiff("src/module_d.ts", "d1", "d2")], { result: "fifth edit ok" }),
    ];
    const runEvent: ToolEvent = { ...members[0], id: "run-shared", editRun: members };
    await mount(false, <EditRunCard event={runEvent} expanded={false} onToggle={() => {}} />);

    const card = host.querySelector('[data-testid="chat-edit-run"]')!;
    expect(card.getAttribute("aria-label")).toBe("Edited 4 files, +5 −5");
    expect(host.querySelector('[data-testid="chat-edit-run-title"]')?.textContent).toBe("Edited 4 files");
    expect(host.querySelector('[data-testid="chat-edit-run-files"]')?.getAttribute("aria-label")).toBe("5 file changes");
    expect(host.querySelector('[data-testid="chat-edit-run-more"]')?.textContent).toContain("Show all 2 remaining files");
    expect(host.querySelectorAll('[data-testid="toolcard-result"]')).toHaveLength(0);

    await act(async () => root!.render(<Surface><ChatBlockHostProvider value={{ renderMenu: renderChatCardMenu, askAbout: () => {} }}><EditRunCard event={runEvent} expanded onToggle={() => {}} /></ChatBlockHostProvider></Surface>));
    expect(host.querySelector('[data-testid="chat-edit-run-title"]')?.textContent).toBe("Edited 4 files");
    expect(host.querySelectorAll('[data-testid="chat-edit-run-file"]')).toHaveLength(5);
  });

  it("keeps tool detail and result behind the group menu even with a file expanded", async () => {
    const notice = "ACP tool details were shortened to fit the display budget.";
    const members: ToolEvent[] = [
      editMember("multi-op", [modifyDiff("src/a.ts", "a1", "a2"), modifyDiff("src/b.ts", "b1", "b2")], {
        detail: notice, result: "multi-file edit succeeded",
      }),
      editMember("dedup-op", [modifyDiff("src/c.ts", "c1", "c2")], {
        detail: notice, result: `${notice}  `,
      }),
      editMember("empty-op", [modifyDiff("src/d.ts", "d1", "d2")]),
      editMember("err-op", [modifyDiff("src/e.ts", "e1", "e2")], {
        status: "error", result: "permission refused",
      }),
    ];
    const runEvent: ToolEvent = { ...members[0], id: "run-results", status: "error", editRun: members };

    await mount(false, <EditRunCard event={runEvent} expanded={false} onToggle={() => {}} />);
    const collapsedButtons = [...host.querySelectorAll<HTMLButtonElement>('[data-testid="chat-edit-run-file"] > div > button')];
    expect(collapsedButtons).toHaveLength(3);

    await act(async () => collapsedButtons[0].click());
    expect(host.textContent).toContain("+a2");
    expect(host.textContent).not.toContain(notice);
    await act(async () => collapsedButtons[1].click());
    expect(host.textContent).toContain("+b2");
    expect(host.textContent).not.toContain("multi-file edit succeeded");

    await act(async () => root!.render(<Surface><ChatBlockHostProvider value={{ renderMenu: renderChatCardMenu, askAbout: () => {} }}><EditRunCard event={runEvent} expanded onToggle={() => {}} /></ChatBlockHostProvider></Surface>));
    const expandedButtons = [...host.querySelectorAll<HTMLButtonElement>('[data-testid="chat-edit-run-file"] > div > button')];
    expect(expandedButtons).toHaveLength(5);
    expect(host.textContent).not.toContain("permission refused");
    expect(await menu("Actions for Edited 5 files")).toContain("View raw");
    await choose("View raw");
    const raw = () => [...host.querySelectorAll('[data-testid="message-raw-content"]')].map((node) => node.textContent);
    expect(raw()).toEqual([notice, "multi-file edit succeeded", notice, "permission refused"]);
    await menu("Actions for Edited 5 files");
    await choose("View raw");
    expect(raw()).toEqual([]);
  });
});
