import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { RegionLoader } from "./RegionLoader";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("renders the shared activity dots with its caption, behind the deferred reveal, never the brand", async () => {
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<RegionLoader label="Loading plans" data-testid="region" />));
    const status = host.querySelector('[role=status][aria-label="Loading plans"]')!;
    expect(status.textContent).toContain("Loading plans");
    expect(status.querySelector(".activity-dots")).not.toBeNull();
    expect(host.querySelector("canvas")).toBeNull();
    expect(host.querySelector('[data-testid="region"]')?.classList.contains("ct-deferred-loader")).toBe(true);
  } finally { await act(async () => root.unmount()); }
});
