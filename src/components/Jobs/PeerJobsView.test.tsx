import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { PairedPeer } from "../../lib/types";
import type { PeerJobsData } from "./peerJobsData";

const { readPeerJobs } = vi.hoisted(() => ({ readPeerJobs: vi.fn() }));
vi.mock("./peerJobsData", () => ({ readPeerJobs }));

import { PeerJobsView } from "./PeerJobsView";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PEER = [{ instance_id: "peer-1", instance_name: "Remote machine", online: true }] as PairedPeer[];
const DATA: PeerJobsData = { workspaces: [], teams: [], resources: [], jobs: [], partial: false };
const CLIENT = (() => ({}) as never);

async function renderView() {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<PeerJobsView peerId="peer-1" peers={PEER} onMachineChange={() => undefined} clientForPeer={CLIENT} />));
  for (let tick = 0; tick < 8; tick++) await act(async () => Promise.resolve());
  return { host, root, close: async () => { await act(async () => root.unmount()); host.remove(); } };
}

afterEach(() => vi.clearAllMocks());

it("keeps automatic load failures as an unavailable status, not an alert", async () => {
  readPeerJobs.mockRejectedValueOnce(new Error("peer offline"));
  const view = await renderView();
  expect(view.host.querySelector('[role="status"]')).not.toBeNull();
  expect(view.host.querySelector('button[aria-label="Refresh jobs"]')).not.toBeNull();
  expect(view.host.querySelector('[role="alert"]')).toBeNull();
  await view.close();
});

it("announces one failed explicit jobs refresh", async () => {
  readPeerJobs.mockResolvedValueOnce(DATA).mockRejectedValueOnce(new Error("refresh denied"));
  const view = await renderView();
  const refresh = view.host.querySelector<HTMLButtonElement>('button[aria-label="Refresh jobs"]');
  expect(refresh).not.toBeNull();
  await act(async () => refresh!.click());
  for (let tick = 0; tick < 8; tick++) await act(async () => Promise.resolve());
  expect(view.host.querySelectorAll('[role="alert"]')).toHaveLength(1);
  await view.close();
});
