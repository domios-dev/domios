import "server-only";
import { cookies } from "next/headers";
import { forbidden } from "next/navigation";
import { getBackend } from "../adapters";
import { requireAdmin, SESSION_COOKIE } from "../domain/auth";
import { AdminError } from "../domain/model";
import { AdminService } from "../domain/service";

export function adminService() {
  const plans = (process.env.ADMIN_PLANS ?? "pro,team").split(",").map(plan => plan.trim()).filter(Boolean);
  return new AdminService(getBackend(), plans);
}
export async function sessionCookie() { return (await cookies()).get(SESSION_COOKIE)?.value; }
export async function pageAdmin() {
  try { return await requireAdmin(getBackend().auth, await sessionCookie()); }
  catch (error) {
    if (error instanceof AdminError && error.status === 403) forbidden();
    throw error;
  }
}
export async function pageResult<T>(operation: () => Promise<T>): Promise<T> {
  try { return await operation(); }
  catch (error) {
    if (error instanceof AdminError && error.status === 403) forbidden();
    throw error;
  }
}
