import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { NeighborReceiveHere } from "./NeighborReceiveHere";
import { NeighborRuntimeFailure, type ReceiverState } from "./neighborRuntimeClient";

vi.mock("../MeshPanel/AddPeerPanel", () => ({ default: () => <div role="region" aria-label="Manage machines">Machine controls</div> }));
vi.mock("../../lib/bridge", () => ({ listen: vi.fn() }));
vi.mock("../../lib/webApi", () => ({ localFetch: vi.fn() }));

let unmount: (() => void) | undefined;
afterEach(() => { unmount?.(); unmount = undefined; document.body.replaceChildren(); });
const receiver: ReceiverState = { receiving_here: false, receiving_enabled: true, receiver_machine_label: "Old desk", revision: 9, source: "friend" };
const button = (name: string) => Array.from(document.querySelectorAll("button")).find(node => node.textContent === name)!;

it("confirms only the named friend and target before calling its bound action", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); unmount = () => act(() => root.unmount());
  const receive = vi.fn(async () => {});
  await act(async () => root.render(<NeighborReceiveHere receiver={receiver} friendName="Ada" targetMachine="New desk" onReceive={receive} />));
  await act(async () => button("Receive this friend here").click());
  expect(receive).not.toHaveBeenCalled();
  expect(document.body.textContent).toContain("Receive Ada's messages on New desk");
  expect(document.body.textContent).not.toContain("all friends");
  await act(async () => button("Receive this friend here").click());
  expect(receive).toHaveBeenCalledExactlyOnceWith(9);
});

it("reset reconfirms when a lower revision arrives and then passes that exact revision", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); unmount = () => act(() => root.unmount());
  const receive = vi.fn(async () => {});
  const show = (revision: number) => <NeighborReceiveHere receiver={{ ...receiver, revision }} friendName="Ada" targetMachine="New desk" operation="reset" initiallyOpen onReceive={receive} />;
  await act(async () => root.render(show(9)));
  await act(async () => root.render(show(2)));
  await act(async () => button("Use account receiver").click());
  expect(receive).not.toHaveBeenCalled();
  await act(async () => button("Use account receiver").click());
  expect(receive).toHaveBeenCalledExactlyOnceWith(2);
});

it("machine-slot refusal shows the counts and exposes existing machine-removal controls", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); unmount = () => act(() => root.unmount());
  const receive = vi.fn(async () => { throw new NeighborRuntimeFailure("key_limit", "opaque", 5, undefined, 5); });
  await act(async () => root.render(<NeighborReceiveHere receiver={receiver} friendName="Ada" initiallyOpen onReceive={receive} />));
  await act(async () => button("Receive this friend here").click());
  expect(document.querySelector('[role="alert"]')?.textContent).toContain("5 of 5 machines in use");
  await act(async () => button("Manage machines").click());
  expect(document.querySelector('[aria-label="Manage machines"]')).not.toBeNull();
});
