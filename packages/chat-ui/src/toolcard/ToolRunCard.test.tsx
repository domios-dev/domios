import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";
import { reduceToolEvents, type NormalizedChatMessage } from "@codeterm/chat-core";
import { MessageBubble } from "../MessageBubble";
import { ChatBlockHostProvider } from "../blocks/ChatBlockHost";
import { contrastRatio, DEFAULT_THEME, themeToCSS } from "../../../../src/lib/themeCss";
import type { ThemeConfig } from "../../../../src/lib/types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function computerStep(
  session: string,
  index: number,
  actionSummary: string,
  options: { error?: boolean; running?: boolean; interactive?: NormalizedChatMessage["interactive"] } = {},
): NormalizedChatMessage[] {
  const toolId = `tu_${session}_${index}`;
  const call: NormalizedChatMessage = {
    id: `${session}-call-${index}`,
    sourceSessionId: session,
    type: "tool_call",
    content: "mcp__claude-in-chrome__computer",
    toolId,
    toolName: "mcp__claude-in-chrome__computer",
    toolKind: "generic",
    toolSummary: "mcp__claude-in-chrome__computer",
    toolDetail: "mcp__claude-in-chrome__computer",
    toolInput: { action: "left_click", action_summary: actionSummary },
    ...(options.interactive ? { interactive: options.interactive } : {}),
  };
  if (options.running) return [call];
  return [
    call,
    {
      id: `${session}-result-${index}`,
      sourceSessionId: session,
      type: "tool_result",
      content: "",
      toolId,
      toolResult: options.error ? `Error in step ${index}: element not found` : `OK step ${index}`,
      toolError: options.error ?? false,
    },
  ];
}

function Timeline({ messages }: { messages: NormalizedChatMessage[] }) {
  return (
    <ChatBlockHostProvider
      value={{
        renderMenu: ({ items }) => (
          <>
            {items.map((item) => (
              <button key={item.label} type="button" onClick={item.onSelect}>
                {item.label}
              </button>
            ))}
          </>
        ),
      }}
    >
      {reduceToolEvents(messages).map((item) => {
        const message = item.kind === "message" ? item.message : item.event.message;
        return (
          <MessageBubble
            key={item.kind === "message" ? `m:${message.id}` : `t:${item.event.id}`}
            message={message}
            item={item.kind === "tool" ? item : undefined}
          />
        );
      })}
    </ChatBlockHostProvider>
  );
}

let root: Root | null = null;
let host: HTMLElement | null = null;

afterEach(async () => {
  if (root) await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

async function renderTimeline(messages: NormalizedChatMessage[]): Promise<HTMLElement> {
  if (!host) {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  }
  await act(async () => root?.render(<Timeline messages={messages} />));
  return host;
}

async function click(element: Element | null | undefined) {
  expect(element).toBeTruthy();
  await act(async () => (element as HTMLElement).click());
}

describe("ToolRunCard UI behaviour", () => {
  it("collapses repeated tool calls into '<tool> × N' and expands/collapses via its accessible name", async () => {
    const s = "expand-collapse";
    const messages = [
      ...computerStep(s, 1, "Open analytics funnel"),
      ...computerStep(s, 2, "Select pricing_viewed"),
      ...computerStep(s, 3, "Save changes"),
    ];
    const view = await renderTimeline(messages);

    const run = view.querySelector<HTMLElement>('[data-testid="chat-tool-run"]');
    expect(run).not.toBeNull();
    expect(run?.dataset.expanded).toBe("false");
    expect(run?.querySelector('[data-testid="chat-tool-run-title"]')?.textContent).toBe("mcp__claude-in-chrome__computer × 3");

    const toggle = run?.querySelector<HTMLButtonElement>('button[aria-label="Expand mcp__claude-in-chrome__computer × 3"]');
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    expect(run?.querySelector('[data-testid="chat-tool-run-list"]')).toBeNull();

    await click(toggle);
    expect(run?.dataset.expanded).toBe("true");
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    expect(toggle?.getAttribute("aria-label")).toBe("Collapse mcp__claude-in-chrome__computer × 3");

    const summaries = [...(run?.querySelectorAll('[data-testid="chat-tool-run-member-summary"]') ?? [])].map((n) => n.textContent);
    expect(summaries).toEqual(["Open analytics funnel", "Select pricing_viewed", "Save changes"]);

    await click(run?.querySelector('button[aria-label="Show Select pricing_viewed"]'));
    expect(run?.querySelector('[data-testid="chat-tool-call"]')?.textContent).toContain("OK step 2");

    await click(run?.querySelector('button[aria-label="Collapse mcp__claude-in-chrome__computer × 3"]'));
    expect(run?.dataset.expanded).toBe("false");
    expect(run?.querySelector('[data-testid="chat-tool-run-list"]')).toBeNull();
  });

  it("shows live running state and keeps expanded state across streaming appends", async () => {
    const s = "live-streaming";
    const initial = [
      ...computerStep(s, 1, "Open page"),
      ...computerStep(s, 2, "Click button", { running: true }),
    ];
    const view = await renderTimeline(initial);
    const run = view.querySelector<HTMLElement>('[data-testid="chat-tool-run"]');
    expect(run?.dataset.toolStatus).toBe("pending");
    expect(run?.querySelector('[data-testid="chat-tool-run-running"]')?.textContent).toBe("running");

    await click(run?.querySelector('[data-testid="chat-tool-run-toggle"]'));
    expect(run?.dataset.expanded).toBe("true");

    await renderTimeline([
      ...computerStep(s, 1, "Open page"),
      ...computerStep(s, 2, "Click button"),
      ...computerStep(s, 3, "Verify toast", { running: true }),
    ]);
    const updated = view.querySelector<HTMLElement>('[data-testid="chat-tool-run"]');
    expect(updated?.dataset.expanded).toBe("true");
    expect(updated?.querySelector('[data-testid="chat-tool-run-title"]')?.textContent).toBe("mcp__claude-in-chrome__computer × 3");
    expect(updated?.querySelector('[data-testid="chat-tool-run-running"]')?.textContent).toBe("running");
  });

  it("keeps errored calls individually visible even while the group is collapsed and shows failure count", async () => {
    const s = "error-visible";
    const messages = [
      ...computerStep(s, 1, "Click Add step"),
      ...computerStep(s, 2, "Rename checkout step", { error: true }),
      ...computerStep(s, 3, "Cancel modal"),
    ];
    const view = await renderTimeline(messages);
    const run = view.querySelector<HTMLElement>('[data-testid="chat-tool-run"]');
    expect(run?.dataset.expanded).toBe("false");
    expect(run?.querySelector('[data-testid="chat-tool-run-failures"]')?.textContent).toBe("1 failed");

    const pinnedErrors = [...(run?.querySelectorAll('[data-testid="chat-tool-run-error-member"]') ?? [])];
    expect(pinnedErrors).toHaveLength(1);
    expect(pinnedErrors[0].textContent).toContain("Rename checkout step");

    await click(pinnedErrors[0].querySelector('button[aria-label="Show Rename checkout step"]'));
    expect(run?.querySelector('[data-testid="chat-tool-call"]')?.textContent).toContain("Error in step 2: element not found");
  });

  it("never hides a tool call with a pending prompt inside a collapsed group", async () => {
    const s = "pending-prompt";
    const messages = [
      ...computerStep(s, 1, "Open form"),
      ...computerStep(s, 2, "Fill fields"),
      ...computerStep(s, 3, "Confirm submission", {
        running: true,
        interactive: [
          {
            type: "question",
            id: "tu_pending_prompt_3-0",
            toolUseId: `tu_${s}_3`,
            prompt: "Submit form?",
            options: [{ label: "Yes" }, { label: "No" }],
            multiSelect: false,
          },
        ],
      }),
    ];
    const view = await renderTimeline(messages);
    expect(view.querySelectorAll('[data-testid="chat-tool-run"]')).toHaveLength(1);
    expect(view.querySelector('[data-testid="chat-tool-run-title"]')?.textContent).toBe("mcp__claude-in-chrome__computer × 2");
    expect(view.querySelectorAll('[data-testid="chat-tool-call"]')).toHaveLength(1);
  });

  it("preserves readable theme tokens across dark and light themes", () => {
    const readTheme = (name: string): ThemeConfig => Object.fromEntries(
      [...readFileSync(resolve(process.cwd(), `src-tauri/themes/${name}.toml`), "utf8").matchAll(/^(\w+) = "([^"]+)"/gm)]
        .map((match) => [match[1], match[2]]),
    ) as unknown as ThemeConfig;
    const required = ["--ct-fg", "--ct-muted-fg", "--ct-surface", "--ct-border", "--ct-accent", "--ct-red", "--ct-yellow"];
    for (const theme of [DEFAULT_THEME, readTheme("github-light")]) {
      const vars = themeToCSS(theme);
      for (const token of required) expect(token in vars).toBe(true);
      expect(contrastRatio(vars["--ct-fg"], vars["--ct-bg"])).toBeGreaterThanOrEqual(7);
      expect(contrastRatio(vars["--ct-red"], vars["--ct-bg"])).toBeGreaterThanOrEqual(4.5);
      expect(contrastRatio(vars["--ct-yellow"], vars["--ct-bg"])).toBeGreaterThanOrEqual(4.5);
    }
  });
});
