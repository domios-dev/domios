import type { NextRequest } from "next/server";
import { SESSION_COOKIE } from "../../../src/domain/auth";
import { adminService } from "../../../src/server/admin";
import { errorResponse } from "../../../src/server/http";
export const runtime = "nodejs";
export async function GET(request: NextRequest) {
  try {
    const user = await adminService().search(request.cookies.get(SESSION_COOKIE)?.value, request.nextUrl.searchParams.get("q") ?? "");
    return Response.json({ user }, { headers: { "Cache-Control": "private, no-store" } });
  } catch (error) { return errorResponse(error); }
}
