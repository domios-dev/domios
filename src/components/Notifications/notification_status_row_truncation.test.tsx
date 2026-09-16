import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { NeedsYouNotificationRow } from "./NeedsYouNotificationRow";
import type { NeedsYouNotificationItem } from "../../lib/notificationDeepLink";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const FULL_TITLE = "opaque-notification-title-token-that-must-remain-accessible";
const mounted: Array<{ root: Root; host: HTMLElement }> = [];

async function render(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mounted.push({ root, host });
  await act(async () => root.render(ui));
  return host;
}

function item(): NeedsYouNotificationItem {
  return {
    key: "notification-key",
    id: "notification-id",
    title: FULL_TITLE,
    state: { kind: "available", tabId: "tab-id", workspaceId: "workspace-id", messageId: "message-id" },
  };
}

afterEach(async () => {
  for (const { root, host } of mounted.splice(0)) {
    await act(async () => root.unmount());
    host.remove();
  }
});

describe("notification status row truncation", () => {
  it("notification_status_row_truncation title carries the truncating treatment and full accessible name", async () => {
    const host = await render(
      <NeedsYouNotificationRow item={item()} tabs={{}} onSelectTab={() => undefined} />,
    );
    const title = host.querySelector<HTMLElement>(".mc-board-needs-title");
    const link = host.querySelector<HTMLElement>('[data-testid="needs-you-notification-link"]');

    expect(title).not.toBeNull();
    expect(link).not.toBeNull();
    expect(title?.classList.contains("mc-board-needs-title")).toBe(true);
    expect(link?.getAttribute("aria-label")).toBe(FULL_TITLE);
  });

  it("notification_status_row_truncation stylesheet declares single-line ellipsis", () => {
    const stylesheet = readFileSync(new URL("../Canvas/missionControl/missionControlBoard.css", import.meta.url), "utf8");
    const rule = stylesheet.match(/\.mc-board-needs-title\s*\{([^}]*)\}/)?.[1] ?? "";

    expect(rule).toMatch(/white-space:\s*nowrap/);
    expect(rule).toMatch(/overflow:\s*hidden/);
    expect(rule).toMatch(/text-overflow:\s*ellipsis/);
  });
});
