import { NextResponse } from "next/server";
import { getBackend } from "../../../src/adapters";
import { COOKIE_OPTIONS, createAdminSession, SESSION_COOKIE, SESSION_SECONDS, requireAdmin } from "../../../src/domain/auth";
import { AdminError } from "../../../src/domain/model";
import { checkOrigin, errorResponse } from "../../../src/server/http";
import type { NextRequest } from "next/server";
export const runtime = "nodejs";

export async function POST(request: Request) {
  try {
    checkOrigin(request);
    const body: unknown = await request.json();
    if (!body || typeof body !== "object" || !("token" in body) || typeof body.token !== "string" || body.token.length > 16000) {
      throw new AdminError(400, "Invalid sign-in token");
    }
    const cookie = await createAdminSession(getBackend().auth, body.token);
    const response = NextResponse.json({ signedIn: true });
    response.cookies.set(SESSION_COOKIE, cookie, { ...COOKIE_OPTIONS, maxAge: SESSION_SECONDS });
    return response;
  } catch (error) { return errorResponse(error); }
}
export async function DELETE(request: NextRequest) {
  try {
    checkOrigin(request);
    await requireAdmin(getBackend().auth, request.cookies.get(SESSION_COOKIE)?.value);
    const response = NextResponse.json({ signedOut: true });
    response.cookies.set(SESSION_COOKIE, "", { ...COOKIE_OPTIONS, maxAge: 0 });
    return response;
  } catch (error) { return errorResponse(error); }
}
