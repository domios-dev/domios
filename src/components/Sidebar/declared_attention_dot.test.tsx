import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { Notification, Tab } from "../../lib/types";
import { TabItem } from "./TabItem";
import {
  declaredAttentionByTab,
  declaredAttentionLabel,
  type DeclaredAttentionLevel,
} from "./declaredAttention";

function notification(
  id: string,
  tabId: string | null,
  level: Notification["level"],
  read = false,
): Notification {
  return {
    id,
    tab_id: tabId,
    workspace_id: "workspace",
    title: id,
    body: null,
    severity: "Info",
    level,
    timestamp: Number(id.replace(/\D/g, "")) || 1,
    read,
  };
}

function renderTab(level: DeclaredAttentionLevel | null): string {
  const tab: Tab = {
    id: "agent-tab",
    pty_id: null,
    tab_type: "Terminal",
    title: "Agent",
    cwd: null,
  };
  return renderToStaticMarkup(
    <TabItem
      tab={tab}
      index={0}
      isActive={false}
      onClick={() => undefined}
      onClose={() => undefined}
      canClose
      workspaceId="workspace"
      declaredAttention={level}
    />,
  );
}

function projection(notifications: Notification[], tabId = "agent-tab") {
  return declaredAttentionByTab(notifications)[tabId] ?? null;
}

describe("declared_attention_dot", () => {
  it("a filed action_needed declaration produces a yellow accessible dot", () => {
    const level = projection([notification("n1", "agent-tab", "action_needed")]);
    const html = renderTab(level);
    expect(level).toBe("action_needed");
    expect(html).toContain("var(--ct-yellow");
    expect(html).toContain(`aria-label="${declaredAttentionLabel("action_needed")}"`);
  });

  it("a filed failure declaration produces the same yellow attention state", () => {
    const level = projection([notification("n2", "agent-tab", "failure")]);
    const html = renderTab(level);
    expect(level).toBe("failure");
    expect(html).toContain("var(--ct-yellow");
    expect(html).toContain(`aria-label="${declaredAttentionLabel("failure")}"`);
  });

  it("a verbose notification produces no dot", () => {
    const level = projection([notification("n3", "agent-tab", "verbose")]);
    expect(level).toBe(null);
    expect(renderTab(level)).not.toContain("Agent declared");
  });

  it("the common case where a tab filed nothing produces no dot", () => {
    const level = projection([]);
    expect(level).toBe(null);
    expect(renderTab(level)).not.toContain("Agent declared");
  });

  it("marking the declaration read clears the dot", () => {
    const open = notification("n4", "agent-tab", "action_needed");
    expect(projection([open])).toBe("action_needed");
    expect(projection([{ ...open, read: true }])).toBe(null);
  });

  it("a declaration marks only the tab that filed it", () => {
    const notifications = [notification("n5", "agent-tab", "action_needed")];
    expect(projection(notifications, "agent-tab")).toBe("action_needed");
    expect(projection(notifications, "other-tab")).toBe(null);
  });
});

finish();
