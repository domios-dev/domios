import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { permissionResponse } from "./neighborRuntime.testdata";
import { NeighborReceiveHere } from "./NeighborReceiveHere";
import { NeighborRuntimeFailure, type ReceiverState } from "./neighborRuntimeClient";

vi.mock("../MeshPanel/AddPeerPanel", () => ({ default: () => <div role="region" aria-label="Manage machines">Machine controls</div> }));
vi.mock("../../lib/bridge", () => ({ listen: vi.fn() }));
vi.mock("../../lib/webApi", async actual => ({ ...await actual<object>(), localFetch: vi.fn() }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let unmount: (() => void) | undefined;
afterEach(() => { unmount?.(); unmount = undefined; document.body.replaceChildren(); });
const receiver: ReceiverState = { receiving_here: false, is_me: false, receiving_enabled: true, receiver_machine_label: "Old desk", revision: 9 };
const preview = { revision: 9, previous_machine: "Old desk", local: permissionResponse(), previous: null, different: false, digest: "snapshot", unavailable: true };
const client = { receiverGrants: vi.fn(async () => preview) };
const button = (name: string) => Array.from(document.querySelectorAll("button")).find(node => node.textContent === name)!;

it("confirms only the named friend and target before calling its bound action", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); unmount = () => act(() => root.unmount());
  const receive = vi.fn(async () => {});
  await act(async () => root.render(<NeighborReceiveHere friendship="friend" client={client} receiver={receiver} friendName="Ada" targetMachine="New desk" onReceive={receive} />));
  expect(document.body.textContent).toContain("Ada is received on Old desk. Move here?");
  await act(async () => button("Receive this friend here").click());
  expect(receive).not.toHaveBeenCalled();
  expect(document.body.textContent).toContain("Ada is received on Old desk. Move here?");
  expect(document.body.textContent).not.toContain("all friends");
  await act(async () => button("Receive this friend here").click());
  expect(receive).toHaveBeenCalledExactlyOnceWith(9, { choice: "keep", expected_digest: "snapshot" });
});

it("reset reconfirms when a newer revision arrives and then passes that exact revision", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); unmount = () => act(() => root.unmount());
  const receive = vi.fn(async () => {});
  const show = (revision: number) => <NeighborReceiveHere friendship="friend" client={client} receiver={{ ...receiver, revision }} friendName="Ada" targetMachine="New desk" operation="reset" initiallyOpen onReceive={receive} />;
  await act(async () => root.render(show(9)));
  await act(async () => root.render(show(10)));
  await act(async () => button("Stop receiving this friend").click());
  expect(receive).not.toHaveBeenCalled();
  await act(async () => button("Stop receiving this friend").click());
  expect(receive).toHaveBeenCalledExactlyOnceWith(10);
});

it("machine-slot refusal shows the counts and exposes existing machine-removal controls", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); unmount = () => act(() => root.unmount());
  const receive = vi.fn(async () => { throw new NeighborRuntimeFailure("key_limit", "opaque", 5, undefined, 5); });
  await act(async () => root.render(<NeighborReceiveHere friendship="friend" client={client} receiver={receiver} friendName="Ada" initiallyOpen onReceive={receive} />));
  await act(async () => button("Receive this friend here").click());
  expect(document.querySelector('[role="alert"]')?.textContent).toContain("5 of 5 machines in use");
  await act(async () => button("Manage machines").click());
  expect(document.querySelector('[aria-label="Manage machines"]')).not.toBeNull();
});

it("invalidates an open confirmation when the named machine changes at the same revision", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); unmount = () => act(() => root.unmount());
  const receive = vi.fn(async () => {});
  const show = (machine: string) => <NeighborReceiveHere friendship="friend" client={client} receiver={{ ...receiver, receiver_machine_label: machine }} friendName="Ada" initiallyOpen onReceive={receive} />;
  await act(async () => root.render(show("Studio")));
  await act(async () => root.render(show("Laptop")));
  await act(async () => button("Receive this friend here").click());
  expect(receive).not.toHaveBeenCalled();
  await act(async () => button("Receive this friend here").click());
  expect(receive).toHaveBeenCalledExactlyOnceWith(9, { choice: "keep", expected_digest: "snapshot" });
});


it.each(["keep", "adopt"] as const)("requires an explicit %s choice and applies it once with the reviewed snapshot", async choice => {
  const previous = permissionResponse({ allowed: true, machine_access: "full", scope: choice === "adopt" ? "Read-only commands. Share the joint project" : "Share the joint project",
    folder_enforcement: { writes: "unrestricted", reads: "not_confined", network: "not_confined", note: "" } });
  const client = { receiverGrants: vi.fn(async () => ({ ...preview, different: true, unavailable: false, previous })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); unmount = () => act(() => root.unmount());
  const receive = vi.fn(async () => {});
  await act(async () => root.render(<NeighborReceiveHere friendship="friend" client={client} receiver={receiver} friendName="Anna" targetMachine="BLACKSTAR" initiallyOpen onReceive={receive} />));
  expect(document.body.textContent).toContain("BLACKSTAR: Disabled · Machine: read_only");
  expect(document.body.textContent).toContain("Old desk: Enabled · Machine: full");
  expect(button("Apply").disabled).toBe(true);
  expect(receive).not.toHaveBeenCalled();
  const radios = document.querySelectorAll<HTMLInputElement>('input[type="radio"]');
  await act(async () => radios[choice === "keep" ? 0 : 1]!.click());
  if (choice === "adopt") {
    expect(button("Apply").disabled).toBe(true);
    const scope = document.querySelector<HTMLTextAreaElement>("textarea")!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(scope, "Share the joint project");
      scope.dispatchEvent(new Event("input", { bubbles: true }));
    });
  }
  expect(button("Apply").disabled).toBe(false);
  await act(async () => button("Apply").click());
  expect(receive).toHaveBeenCalledExactlyOnceWith(9, { choice, expected_digest: "snapshot", ...(choice === "adopt" ? { scope: "Share the joint project" } : {}) });
});

it("keeps receiving unchanged on cancel and on a failed grant comparison", async () => {
  const client = { receiverGrants: vi.fn(async () => { throw new Error("Peer grant read failed"); }) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); unmount = () => act(() => root.unmount());
  const receive = vi.fn(async () => {});
  await act(async () => root.render(<NeighborReceiveHere friendship="friend" client={client} receiver={receiver} friendName="Anna" initiallyOpen onReceive={receive} />));
  expect(document.querySelector('[role="alert"]')?.textContent).toBe("Peer grant read failed");
  expect(button("Receive this friend here").disabled).toBe(true);
  await act(async () => button("Cancel").click());
  expect(receive).not.toHaveBeenCalled();
});

it("retries grant preview and an entitlement refusal without changing the reviewed revision", async () => {
  const client = { receiverGrants: vi.fn(async () => preview).mockRejectedValueOnce(new Error("Preview unavailable")) };
  const receive = vi.fn(async () => {}).mockRejectedValueOnce(new NeighborRuntimeFailure("entitlements_unavailable", "opaque"));
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); unmount = () => act(() => root.unmount());
  await act(async () => root.render(<NeighborReceiveHere friendship="friend" client={client} receiver={receiver} friendName="Ada" initiallyOpen onReceive={receive} />));
  expect(button("Receive this friend here").disabled).toBe(true);
  await act(async () => button("Retry reading permissions").click());
  await act(async () => button("Receive this friend here").click());
  expect(document.querySelector('[role="alert"]')?.textContent).toContain("couldn't verify your account access");
  await act(async () => button("Retry receiving").click());
  expect(receive.mock.calls).toEqual([
    [9, { choice: "keep", expected_digest: "snapshot" }],
    [9, { choice: "keep", expected_digest: "snapshot" }],
  ]);
});
