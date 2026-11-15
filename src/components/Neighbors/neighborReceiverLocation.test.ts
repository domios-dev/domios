import { afterEach, expect, it, vi } from "vitest";
import { localJson } from "../../lib/webApi";
import { nativeBridge } from "../../lib/nativeBridge";
import { openReceiver, receiverOpenItem } from "./neighborReceiverLocation";
import { friendReceiveItems, friendReceivingLine } from "./neighborFriendReceiving";
import type { ReceiverState } from "./neighborRuntimeClient";

vi.mock("../../lib/webApi", async actual => ({
  ...await actual<object>(),
  localJson: vi.fn(), currentToken: () => "opaque-token", selfHost: () => "http://127.0.0.1:7717",
  relayPeerUsable: (peer: { online?: boolean }) => peer?.online === true,
}));
vi.mock("../../lib/nativeBridge", () => ({ nativeBridge: { openUrl: vi.fn(async () => {}) } }));
vi.mock("../../lib/notices", () => ({ publishNotice: vi.fn() }));

afterEach(() => vi.clearAllMocks());

const receiver: ReceiverState = { receiving_here: false, receiving_enabled: true, receiver_machine_label: "Desk", revision: 8, is_me: false };
const location = { receiver_installation_id: "receiver-installation", receiver_instance_id: "receiver-instance", receiver_online: true };

it("retains the receiver location offline and offers an unavailable open action alongside explicit receive-here", () => {
  const offline = { ...receiver, ...location, receiver_online: false };
  expect(friendReceivingLine(offline)).toBe("receiving on Desk · offline");
  const item = receiverOpenItem(offline, "Desk");
  expect(item).toMatchObject({ label: "Open on Desk", unavailable: true });
  item?.onClick?.();
  expect(localJson).not.toHaveBeenCalled();
  expect(nativeBridge.openUrl).not.toHaveBeenCalled();
  const confirm = vi.fn();
  const receive = friendReceiveItems(offline, confirm).find(entry => entry.label === "Receive this friend here");
  expect(confirm).not.toHaveBeenCalled();
  receive?.onClick?.();
  expect(confirm).toHaveBeenCalledExactlyOnceWith("set");
});

it("opens the exact receiver instance even when another machine has the same label", async () => {
  vi.mocked(localJson).mockResolvedValueOnce({ installations: [
    { installation_id: "other-installation", name: "Desk", active: true, signed_out: false, instance_id: "other-instance" },
    { installation_id: "receiver-installation", name: "Desk", active: true, signed_out: false, instance_id: "receiver-instance" },
  ], paired: [{ instance_id: "receiver-instance", online: true }] }).mockResolvedValueOnce({ instance_id: "gateway-instance" });
  await openReceiver("receiver-installation", "receiver-instance");
  expect(nativeBridge.openUrl).toHaveBeenCalledExactlyOnceWith("http://127.0.0.1:7717#token=opaque-token&relay=receiver-instance");
});

it("refuses an old destination when that installation now maps to another instance", async () => {
  vi.mocked(localJson).mockResolvedValueOnce({ installations: [
    { installation_id: "receiver-installation", active: true, signed_out: false, instance_id: "replacement-instance" },
  ], paired: [{ instance_id: "receiver-instance", online: true }] }).mockResolvedValueOnce({ instance_id: "gateway-instance" });
  await expect(openReceiver("receiver-installation", "receiver-instance")).rejects.toThrow("connection changed");
  expect(nativeBridge.openUrl).not.toHaveBeenCalled();
});

it("leaves an unknown concierge location visible without inventing an open destination", () => {
  expect(friendReceivingLine(null)).toBe("concierge location unknown");
  expect(receiverOpenItem(receiver, "Desk")).toBeNull();
});
