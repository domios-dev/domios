import { expect, it, vi } from "vitest";
import { localFetch } from "../../lib/webApi";
import { runtimeCall } from "./neighborRuntimeClient";

vi.mock("../../lib/webApi", () => ({ localFetch: vi.fn() }));
vi.mock("../../lib/bridge", () => ({ listen: vi.fn() }));
vi.mock("../../lib/nativeBridge", () => ({ nativeBridge: { window: undefined, openFolder: vi.fn() } }));

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
  await expect(runtimeCall({ operation: "set_permission" })).rejects.toMatchObject({ reason: "receiving_elsewhere" });
});
