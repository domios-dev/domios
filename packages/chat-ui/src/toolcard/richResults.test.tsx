import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  reduceToolEvents,
  type NormalizedChatMessage,
  type TimelineItem,
  type ToolEventKind,
  type ToolRichResult,
} from "@codeterm/chat-core";
import { ToolCallBubble } from "../ToolCallBubble";
import { configureLocalArtifactLoader } from "../localArtifactLoader";
import { DEFAULT_THEME, themeToCSS } from "../../../../src/lib/themeCss";
import { completedActivity, type ActivityMessage } from "../../../../plugins/provider-codex/src/activity";
import { imageGenerationEndResult, webSearchEndResult } from "../../../../plugins/provider-codex/src/richResults";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const TS = "2026-01-02T03:04:05Z";
const PNG = "data:image/png;base64,iVBORw0KGgo=";
const CODEX_SESSION = "codex-session";

const webEnd = {
  type: "web_search_end", call_id: "ws-1", turn_id: "turn-a", query: "happy-dom img onError dispatch",
  action: { type: "search", query: "happy-dom img onError dispatch" },
  results: [
    { title: "Opaque result title one", url: "https://example.test/1" },
    { title: "Opaque result title two", url: "https://example.test/2" },
    { title: "Opaque result title three", url: "https://example.test/3" },
  ],
};
const imageEnd = {
  type: "image_generation_end", call_id: "ig-1", status: "completed", revised_prompt: "a lighthouse at dusk",
  result: "iVBORw0KGgoOPAQUEBYTES", saved_path: "C:\\Users\\dev\\images\\lighthouse.png",
};
const imageViewItem = { type: "ImageView", id: "view-1", path: "file:///C:/repo/shots/before.png" };
const extensionItem = {
  type: "Extension", kind: "web.search", id: "ext-1", query: "https://example.test/docs",
  action: { type: "openPage", url: "https://example.test/docs" },
  results: [{ type: "text_result", title: "Opaque docs page", thumbnail_url: "https://images.example.test/thumb.png" }],
};

// Mirrors the tab_chat tool_use mapping so fixtures travel the real plugin → ChatMsg → reducer path.
function toChat(activity: ActivityMessage | null, provider = "codex", sourceSessionId = CODEX_SESSION): NormalizedChatMessage {
  if (!activity) throw new Error("expected a parsed record");
  const data = activity.blocks[0].data;
  const rich = data.tool_rich_result;
  return {
    id: activity.uuid, type: "tool_call", content: String(data.tool_summary ?? ""), provider, sourceSessionId,
    toolName: data.name as string, toolId: data.id as string, toolKind: data.tool_kind as ToolEventKind,
    toolSummary: data.tool_summary as string, toolDetail: data.tool_detail as string | undefined,
    toolArgs: data.tool_detail as string | undefined, toolTitle: data.tool_title as string,
    toolFlowId: data.tool_flow_id as string | undefined,
    toolError: data.tool_status === "error" ? true : data.tool_status === "success" ? false : undefined,
    toolRichResult: rich && typeof rich === "object" && !Array.isArray(rich) ? rich as ToolRichResult : undefined,
  };
}

const web = () => toChat(webSearchEndResult(webEnd, 0, TS));
const generated = (patch: Record<string, unknown> = {}) => toChat(imageGenerationEndResult({ ...imageEnd, ...patch }, 0, TS));
const viewed = () => toChat(completedActivity(imageViewItem, 0, TS, "turn-a"));

function boundary(state: "start" | "end", turnId: string): NormalizedChatMessage {
  return { id: `flow-${state}-${turnId}`, type: "tool_flow", content: state, toolFlowId: turnId, sourceSessionId: CODEX_SESSION, seq: 1 };
}

function Row({ item }: { item: TimelineItem }) {
  const [expanded, setExpanded] = useState(false);
  if (item.kind === "message") return <p>{item.message.content}</p>;
  return <ToolCallBubble event={item.event} expanded={expanded} onToggle={setExpanded} />;
}

function Timeline({ messages }: { messages: NormalizedChatMessage[] }) {
  return <>{reduceToolEvents(messages).map((item) => (
    <Row key={item.kind === "message" ? item.message.id : item.event.id} item={item} />
  ))}</>;
}

const makeLoader = () => vi.fn(async (path: string, _kind: string) => {
  if (path.includes("missing")) throw new Error("Unable to resolve Local preview file");
  return { data: PNG, media_type: "image/png" };
});
const makeFetch = () => vi.fn((..._args: unknown[]): never => { throw new Error("network access is forbidden"); });

const mounted: { root: Root; host: HTMLElement }[] = [];
let loader: ReturnType<typeof makeLoader>;
let fetchSpy: ReturnType<typeof makeFetch>;

beforeEach(() => {
  loader = makeLoader();
  configureLocalArtifactLoader(loader);
  fetchSpy = makeFetch();
  vi.stubGlobal("fetch", fetchSpy);
});

afterEach(async () => {
  for (const { root, host } of mounted.splice(0)) {
    await act(async () => root.unmount());
    host.remove();
  }
  configureLocalArtifactLoader(null);
  vi.unstubAllGlobals();
  expect(fetchSpy).not.toHaveBeenCalled();
});

async function render(messages: NormalizedChatMessage[]): Promise<HTMLElement> {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mounted.push({ root, host });
  await act(async () => root.render(<Timeline messages={messages} />));
  await flush();
  return host;
}

async function flush() {
  await act(async () => { await Promise.resolve(); });
  await act(async () => { await Promise.resolve(); });
}

async function click(element: Element | null | undefined) {
  expect(element).toBeTruthy();
  await act(async () => (element as HTMLElement).click());
  await flush();
}

const rows = (scope: ParentNode) => [...scope.querySelectorAll<HTMLElement>('[data-testid="chat-rich-result"]')];
const toggle = (row: HTMLElement) => row.querySelector<HTMLButtonElement>("button[aria-controls]");
const field = (row: HTMLElement, name: string) => row.querySelector(`dd[data-field="${name}"]`)?.textContent;
const summary = (row: HTMLElement) => row.querySelector('[data-testid="chat-rich-result-summary"]')?.textContent;

describe("web results", () => {
  it("renders web_search_end as a compact query and result-count row without result content", async () => {
    const view = await render([web()]);
    const [row] = rows(view);
    expect(rows(view)).toHaveLength(1);
    expect(row.dataset.richKind).toBe("web");
    expect(summary(row)).toBe(webEnd.query);
    expect(row.querySelector('[data-testid="chat-rich-result-count"]')?.textContent).toBe("3 results");
    expect(view.textContent).not.toContain("Opaque result title");
    expect(view.querySelector("img")).toBeNull();
    expect(loader).not.toHaveBeenCalled();

    await click(toggle(row));
    expect(field(row, "query")).toBe(webEnd.query);
    expect(field(row, "results")).toBe("3");
    expect(field(row, "action")).toBe("search");
    expect(view.textContent).not.toContain("Opaque result title");
  });

  it("says the count is not reported when the source carries no results", async () => {
    const { results: _results, ...uncounted } = webEnd;
    const view = await render([toChat(webSearchEndResult(uncounted, 0, TS))]);
    const [row] = rows(view);
    expect(row.querySelector('[data-testid="chat-rich-result-count"]')).toBeNull();
    await click(toggle(row));
    expect(field(row, "results")).toBe("not reported");
  });

  it("renders an Extension web lookup with exact query, action, url, and count and no remote thumbnail", async () => {
    const view = await render([toChat(completedActivity(extensionItem, 0, TS, "turn-a"))]);
    const [row] = rows(view);
    expect(summary(row)).toBe(extensionItem.query);
    expect(row.querySelector('[data-testid="chat-rich-result-count"]')?.textContent).toBe("1 result");
    await click(toggle(row));
    expect(field(row, "query")).toBe(extensionItem.query);
    expect(field(row, "action")).toBe("openPage");
    expect(field(row, "url")).toBe(extensionItem.action.url);
    expect(view.querySelector("img")).toBeNull();
    expect(view.innerHTML).not.toContain("images.example.test");
    expect(view.textContent).not.toContain("Opaque docs page");
  });
});

describe("image results", () => {
  it("shows a local thumbnail for image_generation_end with honest alt text and exact detail", async () => {
    const view = await render([generated()]);
    const [row] = rows(view);
    expect(loader).toHaveBeenCalledTimes(1);
    expect(loader).toHaveBeenCalledWith(imageEnd.saved_path, "image");
    const img = row.querySelector<HTMLImageElement>('[data-testid="chat-rich-result-thumbnail"]');
    expect(img?.getAttribute("src")).toBe(PNG);
    expect(img?.getAttribute("alt")).toBe("Generated image: lighthouse.png");
    expect(summary(row)).toBe("lighthouse.png");
    expect(view.innerHTML).not.toContain(imageEnd.result);

    await click(toggle(row));
    expect(field(row, "source")).toBe("image generation");
    expect(field(row, "path")).toBe(imageEnd.saved_path);
    expect(field(row, "prompt")).toBe(imageEnd.revised_prompt);
    expect(field(row, "status")).toBe("completed");
    expect(field(row, "thumbnail")).toBe("shown from local file");
  });

  it("reads an ImageView file URI as its local path while the detail keeps the exact source path", async () => {
    const view = await render([viewed()]);
    const [row] = rows(view);
    expect(loader).toHaveBeenCalledWith("C:/repo/shots/before.png", "image");
    expect(row.querySelector('[data-testid="chat-rich-result-thumbnail"]')?.getAttribute("alt")).toBe("Viewed image: before.png");
    await click(toggle(row));
    expect(field(row, "source")).toBe("image view");
    expect(field(row, "path")).toBe(imageViewItem.path);
  });

  it.each([
    ["an https URL", "https://example.test/cat.png", "not shown: path is not local, nothing was fetched"],
    ["a network share", "\\\\fileserver\\share\\cat.png", "not shown: path is not local, nothing was fetched"],
    ["a file URI with a host", "file://fileserver/share/cat.png", "not shown: path is not local, nothing was fetched"],
    ["a relative path", "images/cat.png", "not shown: path is not absolute"],
    ["a non-image file", "C:\\repo\\notes.txt", "not shown: not an image file"],
    ["no path", undefined, "not shown: no local path reported"],
  ])("keeps %s as an honest text result without reading or fetching", async (_label, path, note) => {
    const view = await render([generated({ saved_path: path })]);
    const [row] = rows(view);
    expect(loader).not.toHaveBeenCalled();
    expect(view.querySelector("img")).toBeNull();
    await click(toggle(row));
    expect(field(row, "path")).toBe(path ?? "not reported");
    expect(field(row, "thumbnail")).toBe(note);
    expect(view.querySelector("img")).toBeNull();
  });

  it("falls back to text when the local file cannot be read", async () => {
    const view = await render([generated({ saved_path: "C:\\repo\\missing.png" })]);
    const [row] = rows(view);
    expect(loader).toHaveBeenCalledWith("C:\\repo\\missing.png", "image");
    expect(view.querySelector("img")).toBeNull();
    await click(toggle(row));
    expect(field(row, "thumbnail")).toBe("not shown: local file could not be read");
  });

  it("removes a thumbnail that fails to decode instead of leaving a broken image", async () => {
    const view = await render([generated()]);
    const [row] = rows(view);
    const img = row.querySelector('[data-testid="chat-rich-result-thumbnail"]');
    expect(img).not.toBeNull();
    await act(async () => { img?.dispatchEvent(new Event("error")); });
    expect(view.querySelector("img")).toBeNull();
    await click(toggle(row));
    expect(field(row, "thumbnail")).toBe("not shown: local file could not be read");
  });
});

describe("isolation and ordering", () => {
  it("skips malformed records per line and keeps valid neighbours rendering", async () => {
    expect(webSearchEndResult("junk", 0, TS)).toBeNull();
    expect(webSearchEndResult({ call_id: "ws-x" }, 0, TS)).toBeNull();
    expect(webSearchEndResult({ query: "no call id" }, 0, TS)).toBeNull();
    expect(imageGenerationEndResult({ status: "completed" }, 0, TS)).toBeNull();
    expect(completedActivity({ type: "ImageView", id: "v", path: 7 }, 0, TS)).toBeNull();
    expect(completedActivity({ type: "Extension", kind: "web.search", id: "e" }, 0, TS)).toBeNull();
    expect(completedActivity({ ...extensionItem, kind: "other.tool" }, 0, TS)).toBeNull();

    const malformedImage = { ...viewed(), id: "bad-image", toolId: "bad-image", toolRichResult: { kind: "image", path: 42, origin: "sideways" } as unknown as ToolRichResult };
    const garbage = { ...web(), id: "bad-web", toolId: "bad-web", toolRichResult: "garbage" as unknown as ToolRichResult };
    const view = await render([web(), malformedImage, garbage, viewed()]);
    const all = rows(view);
    expect(all).toHaveLength(4);
    expect(summary(all[0])).toBe(webEnd.query);
    expect(all[3].querySelector('[data-testid="chat-rich-result-thumbnail"]')).not.toBeNull();
    expect(loader).toHaveBeenCalledTimes(1);

    await click(toggle(all[1]));
    expect(field(all[1], "path")).toBe("not reported");
    expect(field(all[1], "source")).toBeUndefined();
    expect(all[1].querySelector("img")).toBeNull();
    await click(toggle(all[2]));
    expect(all[2].querySelector('[data-testid="chat-rich-result-unavailable"]')).not.toBeNull();
  });

  it("keeps result rows in source order inside their owning turn stream", async () => {
    const command: NormalizedChatMessage = {
      id: "call-a1", type: "tool_call", content: "npm test", sourceSessionId: CODEX_SESSION, toolName: "exec", toolId: "call-a1",
      toolKind: "command", toolSummary: "npm test", toolDetail: "npm test", toolTitle: "npm test", toolError: false,
    };
    const later = toChat(webSearchEndResult({ ...webEnd, call_id: "ws-2", turn_id: "turn-b", query: "second query" }, 0, TS));
    const messages = [
      boundary("start", "turn-a"), command, web(), viewed(), boundary("end", "turn-a"),
      boundary("start", "turn-b"), later,
    ];
    const stream = reduceToolEvents(messages).find((item) => item.kind === "tool" && item.event.stream);
    expect(stream?.kind === "tool" && stream.event.stream?.members.map((member) => member.id)).toEqual(["call-a1", "result-ws-1", "activity-view-1"]);

    const view = await render(messages);
    const streams = view.querySelectorAll<HTMLElement>('[data-testid="chat-tool-stream"]');
    expect(streams).toHaveLength(1);
    await click(streams[0].querySelector('button[aria-label="Expand 3 actions"]'));
    const labels = [...streams[0].querySelectorAll('[data-testid="chat-tool-stream-action"]')].map((node) => node.textContent?.trim());
    expect(labels).toEqual(["npm test", webEnd.query, "before.png"]);

    await click(streams[0].querySelector('button[aria-label="Show before.png"]'));
    const member = rows(streams[0])[0];
    expect(field(member, "path")).toBe(imageViewItem.path);

    const outside = rows(view).filter((row) => !streams[0].contains(row));
    expect(outside.map(summary)).toEqual(["second query"]);
    expect(streams[0].compareDocumentPosition(outside[0]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});

describe("accessibility and presentation", () => {
  it("exposes a labelled disclosure that toggles a controlled details region", async () => {
    const view = await render([web()]);
    const [row] = rows(view);
    const button = toggle(row);
    const region = view.querySelector<HTMLElement>(`[id="${button?.getAttribute("aria-controls")}"]`);
    expect(button?.getAttribute("aria-expanded")).toBe("false");
    expect(button?.getAttribute("aria-label")).toBe(`Show web result ${webEnd.query}`);
    expect(region?.getAttribute("role")).toBe("region");
    expect(region?.hidden).toBe(true);
    expect(row.querySelector('[data-testid="chat-rich-result-status"]')?.getAttribute("aria-label")).toBe("success");

    await click(button);
    expect(button?.getAttribute("aria-expanded")).toBe("true");
    expect(button?.getAttribute("aria-label")).toBe(`Hide web result ${webEnd.query}`);
    expect(region?.hidden).toBe(false);
  });

  it("keeps motion behind reduced-motion variants, uses theme tokens, and draws no right border or blinking dot", async () => {
    const pending = { ...generated(), id: "pending-image", toolId: "pending-image", toolError: undefined };
    const view = await render([web(), generated(), pending]);
    const animated = [...view.querySelectorAll('[class*="animate-"]')];
    expect(animated.length).toBeGreaterThan(0);
    for (const element of animated) {
      const tokens = (element.getAttribute("class") ?? "").split(/\s+/).filter((token) => token.includes("animate-"));
      expect(tokens.every((token) => token.startsWith("motion-safe:"))).toBe(true);
    }
    expect(view.querySelector('[class*="animate-pulse"], [class*="animate-ping"]')).toBeNull();
    for (const row of rows(view)) {
      expect(row.className.split(/\s+/)).toEqual(expect.not.arrayContaining(["border", "border-r", "border-x"]));
      expect(row.style.borderRightWidth).toBe("");
      expect(row.getAttribute("style")).toContain("var(--ct-");
      expect(toggle(row)?.className).toContain("motion-reduce:transition-none");
    }
    const theme = themeToCSS(DEFAULT_THEME);
    const tokens = new Set([...view.innerHTML.matchAll(/var\((--ct-[a-z-]+)\)/g)].map((match) => match[1]));
    expect(tokens.size).toBeGreaterThan(0);
    for (const token of tokens) expect(token in theme).toBe(true);
  });
});

describe("provider neutrality", () => {
  const html = async (message: NormalizedChatMessage, expand: boolean) => {
    const view = await render([message]);
    if (expand) await click(toggle(rows(view)[0]));
    return view.innerHTML.replace(/\s(?:id|aria-controls)="[^"]*"/g, "");
  };

  it.each([
    ["web", web],
    ["image", viewed],
  ])("renders equivalent normalized Claude and Codex %s result fields identically", async (_kind, build) => {
    const codex = build();
    const claude = { ...codex, provider: "claude", sourceSessionId: "claude-session", toolName: "WebSearch" };
    for (const expand of [false, true]) {
      expect(await html(claude, expand)).toBe(await html(codex, expand));
    }
  });
});
