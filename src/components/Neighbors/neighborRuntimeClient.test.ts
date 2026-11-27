import { expect, it, vi } from "vitest";
import { recordProductEvent } from "../../lib/productAnalytics";
import { clearRelayTarget, localFetch, setRelayTarget, webFetch } from "../../lib/webApi";
import { neighborRuntimeClient, runtimeCall } from "./neighborRuntimeClient";

vi.mock("../../lib/webApi", async actual => ({ ...await actual<object>(), localFetch: vi.fn(), webFetch: vi.fn() }));
vi.mock("../../lib/productAnalytics", async actual => ({ ...await actual<object>(), recordProductEvent: vi.fn(async () => {}) }));
vi.mock("../../lib/bridge", () => ({ listen: vi.fn() }));
vi.mock("../../lib/nativeBridge", () => ({ nativeBridge: { window: undefined, openFolder: vi.fn() } }));

it.each(["set", "reset"] as const)("moves only the requested friend for %s", async operation => {
  vi.mocked(localFetch).mockResolvedValueOnce(new Response("{}", { status: 200 }));
  await neighborRuntimeClient.receiveFriend("f".repeat(64), 9, operation);
  const calls = vi.mocked(localFetch).mock.calls;
  const [, init] = calls[calls.length - 1]!;
  expect(JSON.parse(String(init?.body))).toEqual({ operation: "set_friend_receiver", friendship_id: "f".repeat(64),
    receiver_operation: operation, takeover_confirmed: true, expected_receiver_revision: 9 });
});

it("keeps machine quota metadata and new receiver refusals typed", async () => {
  vi.mocked(localFetch).mockResolvedValueOnce(new Response(JSON.stringify({ code: "key_limit", limit: 5, active: 5, message: "All machine slots are in use" }), { status: 409 }));
  await expect(neighborRuntimeClient.receiveFriend("f".repeat(64), 1)).rejects.toMatchObject({ reason: "key_limit", limit: 5, active: 5 });
  vi.mocked(localFetch).mockResolvedValueOnce(new Response(JSON.stringify({ code: "receiver_revision_stale", message: "Receiving changed; confirm again" }), { status: 409 }));
  await expect(neighborRuntimeClient.receiveFriend("f".repeat(64), 1)).rejects.toMatchObject({ reason: "receiver_revision_stale" });
});

it("opens a saved file from the desktop app so it comes to the front, and falls back to the daemon open", async () => {
  const { openSavedFile } = await import("./neighborFilesClient");
  const desktop = { window: {} as never, openFolder: vi.fn(async () => {}) };
  const runtime = vi.fn(async (command: Record<string, unknown>) => command.launch === false ? { opened: false, path: "C:/n/ada/note.txt" } : { opened: true });
  await openSavedFile("pair", "file-1", runtime, desktop);
  expect(runtime).toHaveBeenCalledExactlyOnceWith({ operation: "open", friendship_id: "pair", file_id: "file-1", launch: false });
  expect(desktop.openFolder).toHaveBeenCalledExactlyOnceWith("C:/n/ada/note.txt");

  // An older daemon refuses launch: the daemon opens it as before.
  const older = vi.fn(async (command: Record<string, unknown>) => { if ("launch" in command) throw new Error("unknown field launch"); return { opened: true }; });
  await openSavedFile("pair", "file-1", older, { window: {} as never, openFolder: vi.fn() });
  expect(older).toHaveBeenLastCalledWith({ operation: "open", friendship_id: "pair", file_id: "file-1" });

  // The web UI on another device has no desktop to open from; the host's daemon opens it.
  const web = vi.fn(async () => ({ opened: true }));
  const webDesktop = { window: undefined, openFolder: vi.fn() };
  await openSavedFile("pair", "file-1", web, webDesktop);
  expect(web).toHaveBeenCalledExactlyOnceWith({ operation: "open", friendship_id: "pair", file_id: "file-1" });
  expect(webDesktop.openFolder).not.toHaveBeenCalled();
});

it("keeps the typed ended reason and daemon diagnostic instead of unavailable", async () => {
  vi.mocked(localFetch).mockResolvedValueOnce(new Response(JSON.stringify({ code: "friendship_ended", message: "The retained conversation has ended" }), { status: 409 }));
  await expect(runtimeCall({ operation: "send" })).rejects.toMatchObject({ reason: "friendship_ended", message: "The retained conversation has ended" });
});

it("preserves plain local failure details", async () => {
  vi.mocked(localFetch).mockResolvedValueOnce(new Response("Neighbor cursor storage unavailable", { status: 503 }));
  await expect(runtimeCall({ operation: "send" })).rejects.toMatchObject({ message: "Neighbor cursor storage unavailable" });
});

it("keeps size and tab-request refusals typed", async () => {
  vi.mocked(localFetch).mockResolvedValueOnce(new Response(JSON.stringify({ code: "message_too_large", limit: 32752, message: "At most 32752 UTF-8 bytes" }), { status: 413 }));
  await expect(runtimeCall({ operation: "send" })).rejects.toMatchObject({ reason: "message_too_large", limit: 32752, message: "At most 32752 UTF-8 bytes" });
  vi.mocked(localFetch).mockResolvedValueOnce(new Response(JSON.stringify({ code: "tab_requests_not_allowed", message: "Tab requests are not allowed" }), { status: 403 }));
  await expect(runtimeCall({ operation: "send" })).rejects.toMatchObject({ reason: "tab_requests_not_allowed" });
  vi.mocked(localFetch).mockResolvedValueOnce(new Response(JSON.stringify({ code: "reply_target_stopped", message: "The owner stopped this request; nothing was sent" }), { status: 409 }));
  await expect(runtimeCall({ operation: "send" })).rejects.toMatchObject({ reason: "reply_target_stopped", message: "The owner stopped this request; nothing was sent" });
  vi.mocked(localFetch).mockResolvedValueOnce(new Response(JSON.stringify({ code: "receiving_elsewhere", message: "Another machine receives for this account" }), { status: 409 }));
  await expect(runtimeCall({ operation: "set_permission" })).rejects.toMatchObject({ reason: "receiving_elsewhere", message: "Another machine receives for this account" });
});

it("preserves a named receiving refusal and a separate none-chosen code", async () => {
  const message = "Messages from Anya are received on Anya's Mac. Receive them here?";
  vi.mocked(localFetch).mockResolvedValueOnce(new Response(JSON.stringify({ code: "receiving_elsewhere", message }), { status: 409 }));
  await expect(runtimeCall({ operation: "set_permission" })).rejects.toMatchObject({ reason: "receiving_elsewhere", message });
  vi.mocked(localFetch).mockResolvedValueOnce(new Response(JSON.stringify({ code: "receiver_not_chosen", message: "No receiver chosen" }), { status: 409 }));
  await expect(runtimeCall({ operation: "set_permission" })).rejects.toMatchObject({ reason: "receiver_not_chosen", message: "No receiver chosen" });
});

it("sends write areas explicitly and defaults them to an empty grant", async () => {
  const { neighborConciergeClient } = await import("./neighborConciergeClient");
  for (const memory_write_areas of [undefined, ["project"]]) {
    vi.mocked(localFetch).mockResolvedValueOnce(new Response("{}", { status: 200 }));
    await neighborConciergeClient.setPermission("pair", { allowed: true, scope: "opaque", allow_tab_requests: false, memory_write_areas });
    const request = vi.mocked(localFetch).mock.lastCall![1]!;
    expect(JSON.parse(String(request.body)).permission.memory_write_areas).toEqual(memory_write_areas ?? []);
  }
});

it("keeps pending key confirmation typed without replaying receiver selection", async () => {
  const message = "Neighbor key preparation awaits inventory confirmation; receiving has not changed";
  vi.mocked(localFetch).mockClear();
  vi.mocked(localFetch).mockResolvedValueOnce(new Response(JSON.stringify({ code: "key_inventory_pending", message }), { status: 409 }));
  await expect(neighborRuntimeClient.receiveFriend("f".repeat(64), 1)).rejects.toMatchObject({ reason: "key_inventory_pending", message });
  expect(localFetch).toHaveBeenCalledTimes(1);
});

it("reads the selected installation's history without touching the origin or its read receipts", async () => {
  const { neighborConciergeClient, markNeighborRead } = await import("./neighborConciergeClient");
  setRelayTarget("installation-a", "A");
  vi.mocked(localFetch).mockClear();
  vi.mocked(webFetch).mockResolvedValueOnce(new Response(JSON.stringify({ entries: [], next_before: "a-history", ended: false }), { status: 200 }));
  try {
    await expect(neighborConciergeClient.history("pair")).resolves.toMatchObject({ next_before: "a-history" });
    expect(webFetch).toHaveBeenCalledWith("/api/neighbors/runtime", expect.objectContaining({ body: JSON.stringify({ operation: "history", friendship_id: "pair" }) }));
    await markNeighborRead("pair", "a-message");
    await expect((await import("./neighborsSwitch")).setNeighborsEnabled(false)).rejects.toThrow("Open this machine directly");
    expect(localFetch).not.toHaveBeenCalled();
  } finally { clearRelayTarget(); }
});

it.each(["keep", "adopt"] as const)("submits the reviewed %s decision in the receiver Apply", async choice => {
  vi.mocked(localFetch).mockResolvedValueOnce(new Response("{}", { status: 200 }));
  const decision = { choice, expected_digest: "reviewed-snapshot", ...(choice === "adopt" ? { scope: "Full access for the joint project" } : {}) };
  await neighborRuntimeClient.receiveFriend("f".repeat(64), 3, "set", decision);
  const [, init] = vi.mocked(localFetch).mock.calls[vi.mocked(localFetch).mock.calls.length - 1]!;
  expect(JSON.parse(String(init?.body))).toEqual({ operation: "set_friend_receiver", friendship_id: "f".repeat(64),
    receiver_operation: "set", takeover_confirmed: true, expected_receiver_revision: 3, grant_decision: decision });
});

it("carries the pending bubble id to the runtime without replay on transport failure", async () => {
  const message_id = "a".repeat(32);
  vi.mocked(localFetch).mockClear();
  vi.mocked(recordProductEvent).mockClear();
  vi.mocked(localFetch).mockResolvedValueOnce(new Response(JSON.stringify({ message_id, status: "pending" })));
  await expect(neighborRuntimeClient.send("pair", "Opaque message", message_id)).resolves.toMatchObject({ message_id });
  expect(localFetch).toHaveBeenCalledExactlyOnceWith("/api/account/neighbors/runtime", expect.objectContaining({
    body: JSON.stringify({ operation: "send", friendship_id: "pair", kind: "question", body: "Opaque message", in_reply_to: null, message_id }),
  }));
  expect(recordProductEvent).toHaveBeenCalledExactlyOnceWith({
    event_type: "neighbor_message_sent",
    event_properties: { count: 1 },
  });
  vi.mocked(localFetch).mockClear();
  vi.mocked(recordProductEvent).mockClear();
  vi.mocked(localFetch).mockResolvedValueOnce(new Response(JSON.stringify({ message_id, status: "not_delivered", reason: "recipient_paused" })));
  await expect(neighborRuntimeClient.send("pair", "Opaque message", message_id)).resolves.toMatchObject({ status: "not_delivered" });
  expect(recordProductEvent).not.toHaveBeenCalled();

  vi.mocked(localFetch).mockClear();
  vi.mocked(recordProductEvent).mockClear();
  vi.mocked(localFetch).mockResolvedValueOnce(new Response(JSON.stringify({ message_id, status: "pending", submission: "unconfirmed" })));
  await expect(neighborRuntimeClient.send("pair", "Opaque message", message_id)).resolves.toMatchObject({ submission: "unconfirmed" });
  expect(recordProductEvent).not.toHaveBeenCalled();

  vi.mocked(localFetch).mockClear();
  vi.mocked(recordProductEvent).mockClear();
  vi.mocked(localFetch).mockRejectedValueOnce(new Error("Connection lost"));
  await expect(neighborRuntimeClient.send("pair", "Opaque message", message_id)).rejects.toThrow("Connection lost");
  expect(localFetch).toHaveBeenCalledTimes(1);
  expect(recordProductEvent).not.toHaveBeenCalled();
});
