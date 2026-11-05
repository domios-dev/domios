import React, { act, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ToolEvent } from "@codeterm/chat-core";
import { renderChatCardMenu } from "../../../src/components/Terminal/ChatCardMenu";
import { ChatBlockHostProvider } from "./blocks/ChatBlockHost";
import { ChatBlockFrame } from "./blocks/ChatBlockFrame";
import { toolCommandShape } from "./codeterm/commandShape";
import { CommandGroupCard } from "./toolcard/CommandGroupCard";
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
