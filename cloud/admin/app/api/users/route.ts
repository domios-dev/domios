import type { NextRequest } from "next/server";
import { SESSION_COOKIE } from "../../../src/domain/auth";
import { adminService } from "../../../src/server/admin";
import { errorResponse } from "../../../src/server/http";
export const runtime = "nodejs";
export async function GET(request: NextRequest) {
  try {
    const cookie = request.cookies.get(SESSION_COOKIE)?.value;
    const params = request.nextUrl.searchParams;
    const result = params.has("q")
      ? { user: await adminService().search(cookie, params.get("q") ?? "") }
      : await adminService().list(cookie, Number(params.get("pageSize") ?? 25), params.get("pageToken") ?? undefined);
    return Response.json(result, { headers: { "Cache-Control": "private, no-store" } });
  } catch (error) { return errorResponse(error); }
}
