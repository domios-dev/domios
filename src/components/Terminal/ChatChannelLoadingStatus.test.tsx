import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { tabActivity } from "../../lib/tabActivity";
import { AgentStateIndicator } from "./AgentStateIndicator";
import { chatChannelLoadingRow } from "./chatBodyVisibility";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("hosts the accessible waiting icon in the existing model row without an extra row", async () => {
  const host = document.createElement("div");
  const root = createRoot(host);
  const label = "Fixture channel pending";
  const render = async (loading: boolean) => {
    const row = chatChannelLoadingRow(loading, { kind: "none" });
    await act(async () => root.render(
      <AgentStateIndicator
        activity={tabActivity({ agent_state: { state: "idle" } })}
        providerName="Fixture provider"
        model="Fixture model"
        renderIdentity={(identity) => <span>{identity}</span>}
        channelLoadingLabel={row === "model" ? label : undefined}
      />,
    ));
  };
  try {
    await render(false);
    const modelRow = host.firstElementChild;
    expect(host.childElementCount).toBe(1);
    await render(true);
    const icon = host.querySelector('[role="img"]');
    expect(icon?.getAttribute("aria-label")).toBe(label);
    expect(icon?.getAttribute("title")).toBe(label);
    expect(icon?.parentElement).toBe(modelRow);
    expect(host.firstElementChild).toBe(modelRow);
    expect(host.childElementCount).toBe(1);
    expect(host.querySelectorAll("div")).toHaveLength(1);
    await render(false);
    expect(host.querySelector('[role="img"]')).toBeNull();
    expect(host.firstElementChild).toBe(modelRow);
    expect(host.childElementCount).toBe(1);
  } finally {
    await act(async () => root.unmount());
  }
});
