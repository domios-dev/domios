"use server";
import { cookies } from "next/headers";
import { redirect } from "next/navigation";
import { revalidatePath } from "next/cache";
import { SESSION_COOKIE, COOKIE_OPTIONS } from "../../src/domain/auth";
import { adminService, pageAdmin, pageResult, sessionCookie } from "../../src/server/admin";
export async function changeEntitlement(form: FormData) {
  const cookie = await sessionCookie();
  const uid = String(form.get("uid") ?? "");
  await pageResult(() => adminService().change(cookie, uid, String(form.get("action") ?? ""),
    String(form.get("plan") ?? ""), String(form.get("reason") ?? "")));
  revalidatePath(`/users/${encodeURIComponent(uid)}`);
  redirect(`/users/${encodeURIComponent(uid)}?saved=1`);
}
export async function signOut() {
  await pageAdmin();
  (await cookies()).set(SESSION_COOKIE, "", { ...COOKIE_OPTIONS, maxAge: 0 });
  redirect("/signin");
}
