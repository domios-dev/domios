import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { AppConfig } from "../../lib/types";

vi.mock("../../lib/bridge", () => ({
  invoke: vi.fn(async (command: string) => command === "list_themes" ? [] : ""),
  listen: vi.fn(async () => () => {}),
}));
vi.mock("../../lib/nativeBridge", () => ({ nativeBridge: {} }));
vi.mock("../../store/multiplexer/hooks", () => ({
  useMuxState: () => ({ workspaces: [], tabs: [], teams: [] }),
  useMultiplexer: () => ({ selectTabByTab: vi.fn() }),
}));
vi.mock("../../lib/webApi", async (original) => ({
  ...await original<typeof import("../../lib/webApi")>(),
  localFetch: vi.fn(async () => new Response(JSON.stringify({
    backend: "local", status: { status: "unlocked" }, items: [], organizations: [], collections: [],
  }))),
}));
vi.mock("../../lib/productAnalytics", () => ({ recordProductEvent: vi.fn(async () => {}) }));

import { SettingsPanel } from "./SettingsPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("opens the secret store from the accessible Settings navigation entry", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const config = {
    theme: { name: "github-dark" },
    api: { enabled: true, port: 7686 },
    integrations: { secrets: { backend: "local", default_organization: "", default_collection: "" } },
  } as AppConfig;
  try {
    await act(async () => root.render(
      <SettingsPanel config={config} onUpdateConfig={async () => {}} onClose={() => {}} restoreNavigation={false} />,
    ));
    const navigation = host.querySelector('[role="navigation"][aria-label="Settings sections"]');
    const entries = [...navigation!.querySelectorAll("button")].filter(button => button.textContent?.startsWith("Secrets"));
    expect(entries).toHaveLength(1);
    await act(async () => entries[0].click());
    const region = [...host.querySelectorAll('[role="region"]')].find(element => {
      const label = element.getAttribute("aria-labelledby");
      return label && document.getElementById(label)?.textContent === "Secrets";
    });
    expect(region).toBeDefined();
    expect(region!.querySelector('select[aria-label="Backend"]')).not.toBeNull();
    expect(region!.querySelector('select[aria-label="Backend"]')?.textContent).toContain("Local secret storage");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
