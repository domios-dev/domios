import { expect, it, vi } from "vitest";
import { localFetch } from "../../lib/webApi";
import { runtimeCall } from "./neighborRuntimeClient";

vi.mock("../../lib/webApi", () => ({ localFetch: vi.fn() }));
vi.mock("../../lib/bridge", () => ({ listen: vi.fn() }));

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
});
