import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { RegionLoader } from "./RegionLoader";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("renders the brand loader with its label and status line, behind the deferred reveal", async () => {
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<RegionLoader label="Loading plans" detail="Reading the index" data-testid="region" />));
    const status = host.querySelector('[role=status][aria-label="Loading plans"]')!;
    expect(status.textContent).toContain("Reading the index");
    expect(status.querySelector("svg")).not.toBeNull();
    expect(status.querySelector("canvas")).not.toBeNull();
    expect(host.querySelector('[data-testid="region"]')?.classList.contains("ct-deferred-loader")).toBe(true);
  } finally { await act(async () => root.unmount()); }
});
