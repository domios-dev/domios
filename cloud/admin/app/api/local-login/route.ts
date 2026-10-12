import { getBackend, providerName } from "../../../src/adapters";
import { LocalBackend } from "../../../src/adapters/local";
import { AdminError } from "../../../src/domain/model";
import { checkOrigin, errorResponse } from "../../../src/server/http";
export const runtime = "nodejs";
export async function POST(request: Request) {
  try {
    if (providerName() !== "local") return new Response(null, { status: 404 });
    checkOrigin(request);
    const body: unknown = await request.json();
    if (!body || typeof body !== "object" || !("email" in body) || typeof body.email !== "string" ||
      !("password" in body) || typeof body.password !== "string" || body.email.length > 320 || body.password.length > 1000) {
      throw new AdminError(400, "Invalid credentials");
    }
    const backend = getBackend();
    if (!(backend instanceof LocalBackend)) throw new Error("Invalid local backend");
    return Response.json({ token: await backend.login(body.email, body.password) });
  } catch (error) { return errorResponse(error); }
}
