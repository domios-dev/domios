import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";
import { ActivityLogRow } from "./ActivityLogRow";
import { reportActivitySummary } from "./reportActivity";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("worker report activity", () => {
  it("extracts the first sentence from plain text and Markdown without a status prefix", () => {
    expect(reportActivitySummary("Plain first sentence. Second sentence.")).toBe("Plain first sentence.");
    expect(reportActivitySummary("In Review: ## Changes\n\nNo source changes. See **details** below.")).toBe("No source changes.");
  });

  it("renders the expanded report with the chat Markdown renderer", async () => {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    const report = "In Review: ## Changes\n\nNo source changes.\n\n- Checked **formatting**\n- Kept `rendering` intact";
    await act(async () => root!.render(<ActivityLogRow kind="worker report" age="now" text={report} />));

    const summary = host.querySelector(".ct-activity-row__text");
    expect(summary?.textContent).toBe("No source changes.");
    expect(summary?.textContent).not.toContain("In Review:");
    expect(summary?.textContent).not.toContain("##");

    await act(async () => host!.querySelector<HTMLButtonElement>("button")!.click());
    const body = host.querySelector(".ct-activity-row__full");
    expect(body?.querySelector("h2")?.textContent).toBe("Changes");
    expect(body?.querySelectorAll("li")).toHaveLength(2);
    expect(body?.querySelector("strong")?.textContent).toBe("formatting");
    expect(body?.querySelector("code")?.textContent).toBe("rendering");
  });
});
