import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it } from "vitest";
import { AccountLimits } from "./AccountLimits";
import type { FreeLimits } from "./freeLimitModel";

const limits: FreeLimits = { enforcing: true, max: { workspaces: 7, teams: 10, "agent.tabs": 70 }, meshMax: 5, memoryMax: 1500 };
let root: Root | undefined;
let host: HTMLDivElement;
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
afterEach(async () => { await act(async () => root?.unmount()); host.remove(); });

async function render(usage = {}, value: FreeLimits = limits) {
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  await act(async () => root!.render(<AccountLimits limits={value} usage={usage} />));
}
const values = () => Array.from(host.querySelectorAll("dd")).map(row => row.textContent);

it("shows observed usage, allowance and remaining capacity for every resource", async () => {
  await render({ workspaces: 3, teams: 2, "agent.tabs": 11, "mesh.machines": 4, "mem.items": 120 });
  expect(values()).toEqual(["3 / 7 (4 left)", "2 / 10 (8 left)", "11 / 70 (59 left)", "4 / 5 (1 left)", "120 / 1500 (1380 left)"]);
});

it("shows unlimited allowance alongside usage without a remaining count", async () => {
  await render({ workspaces: 3 }, { enforcing: false, max: { workspaces: null, teams: null, "agent.tabs": null } });
  expect(values()).toEqual(["3 / Unlimited", "Unlimited", "Unlimited"]);
});

it("shows only the allowance when usage is unavailable and clamps remaining capacity at zero", async () => {
  await render({ teams: 12 });
  expect(values()).toEqual(["7", "12 / 10 (0 left)", "70", "5", "1500"]);
});
