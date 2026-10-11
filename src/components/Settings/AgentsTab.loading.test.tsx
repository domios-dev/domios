import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { AppConfig } from "../../lib/types";

const fixture = vi.hoisted(() => ({ loading: true }));
vi.mock("../../hooks/useAgentProviders", () => ({
  useAgentProviders: () => ({ providers: [], loading: fixture.loading, error: null }),
}));
import { AgentProviderScreen } from "./AgentsTab";

it("wires the provider loading boundary and leaves it when the request settles", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const gone = vi.fn();
  const screen = <AgentProviderScreen providerId="opaque-provider" fullConfig={{} as AppConfig} onUpdateFullConfig={vi.fn()} onProviderGone={gone} />;
  try {
    await act(async () => root.render(screen));
    expect(host.querySelector('[role="status"][aria-label="Loading provider"]')).not.toBeNull();
    expect(gone).not.toHaveBeenCalled();
    fixture.loading = false;
    await act(async () => root.render(<AgentProviderScreen providerId="opaque-provider" fullConfig={{} as AppConfig} onUpdateFullConfig={vi.fn()} onProviderGone={gone} />));
    expect(host.querySelector('[role="status"]')).toBeNull();
    expect(gone).toHaveBeenCalledTimes(1);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
