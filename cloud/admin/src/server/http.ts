import "server-only";
import { AdminError } from "../domain/model";
import { providerName } from "../adapters";

export function checkOrigin(request: Request) {
  const origin = request.headers.get("origin");
  const expected = process.env.ADMIN_ORIGIN ?? new URL(request.url).origin;
  if (!origin || origin !== expected) throw new AdminError(403, "Invalid origin");
  if (providerName() !== "firebase" && !["localhost", "127.0.0.1", "[::1]"].includes(new URL(origin).hostname)) {
    throw new AdminError(403, "Development sign-in requires localhost");
  }
}
export function errorResponse(error: unknown) {
  if (error instanceof AdminError) return Response.json({ error: error.message }, { status: error.status });
  return Response.json({ error: "Request failed" }, { status: 500 });
}
