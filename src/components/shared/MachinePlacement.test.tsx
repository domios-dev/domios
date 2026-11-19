import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { machinePlacementChoices, LOCAL_MACHINE_ID } from "../../lib/machinePlacement";
import { RELAY_PROTOCOL_VERSION, type MeshPeersResponse } from "../../lib/types";
import { MachinePlacement } from "./MachinePlacement";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement;

function render(element: ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  act(() => root!.render(element));
}

afterEach(() => {
  act(() => root?.unmount());
  root = null;
  host?.remove();
});

it("keeps the local machine first and exposes offline and incompatible peers as unavailable", () => {
  const response = {
    local_instance_name: "LOCAL [fixture]",
    local_machine_identity: { kind: "desktop", color: "blue" },
    machine_identities: { offline: { kind: "laptop", color: "teal" } },
    paired: [
      { instance_id: "z-online", instance_name: "Zulu [fixture]", online: true, access_level: "full", relay_protocol_version: RELAY_PROTOCOL_VERSION },
      { instance_id: "offline", instance_name: "Alpha [fixture]", online: false, access_level: "full", relay_protocol_version: RELAY_PROTOCOL_VERSION },
      { instance_id: "old", instance_name: "Older", online: true, access_level: "full", relay_protocol_version: RELAY_PROTOCOL_VERSION - 1 },
    ],
  } as unknown as MeshPeersResponse;

  const choices = machinePlacementChoices(response);

  expect(choices.map(({ id }) => id)).toEqual([LOCAL_MACHINE_ID, "offline", "old", "z-online"]);
  expect(choices[0]).toMatchObject({ name: "LOCAL", local: true, status: "online", unavailableReason: null });
  expect(choices[1].name).toBe("Alpha");
  expect(choices[1].status).toBe("offline");
  expect(choices[1].unavailableReason).toBeTruthy();
  expect(choices[2].status).toBe("needs update");
  expect(choices[2].unavailableReason).toBeTruthy();
});

it("hides the row with one machine and keeps disabled options unavailable while allowing a live peer selection", () => {
  const onChange = vi.fn();
  const choices = machinePlacementChoices({
    local_instance_name: "LOCAL",
    paired: [
      { instance_id: "peer", instance_name: "Peer", online: true, access_level: "full", relay_protocol_version: RELAY_PROTOCOL_VERSION },
      { instance_id: "offline", instance_name: "Offline", online: false, access_level: "full", relay_protocol_version: RELAY_PROTOCOL_VERSION },
    ],
  } as MeshPeersResponse);

  render(<MachinePlacement choices={[choices[0]]} value={LOCAL_MACHINE_ID} onChange={onChange} />);
  expect(host.querySelector('[aria-label^="Where it runs"]')).toBeNull();
  act(() => root!.render(<MachinePlacement choices={choices} value={LOCAL_MACHINE_ID} onChange={onChange} testId="placement" />));

  const trigger = host.querySelector<HTMLButtonElement>('[data-testid="placement"]')!;
  act(() => trigger.dispatchEvent(new Event("pointerdown", { bubbles: true, cancelable: true })));
  const selected = document.body.querySelector<HTMLElement>(`[data-testid="placement-option-${LOCAL_MACHINE_ID}"]`)!;
  expect(selected.getAttribute("role")).toBe("menuitemradio");
  expect(selected.getAttribute("aria-checked")).toBe("true");
  const unavailable = document.body.querySelector<HTMLElement>('[data-testid="placement-option-offline"]')!;
  expect((unavailable as HTMLButtonElement).disabled).toBe(false);
  expect(unavailable.getAttribute("aria-disabled")).toBe("true");
  expect(unavailable.getAttribute("aria-label")).toContain("offline");
  expect(unavailable.textContent?.toLowerCase()).toContain("offline");
  act(() => unavailable.click());
  expect(onChange).not.toHaveBeenCalled();
  const live = document.body.querySelector<HTMLElement>('[data-testid="placement-option-peer"]')!;
  expect(live.getAttribute("aria-label")).toContain("Peer");
  expect(live.getAttribute("aria-label")).toContain("online");
  act(() => live.click());
  expect(onChange).toHaveBeenCalledWith("peer");
});
