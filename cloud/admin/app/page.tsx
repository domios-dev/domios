import { redirect } from "next/navigation";
import { pageAdmin, sessionCookie } from "../src/server/admin";
import SignInPage from "./signin/page";
export default async function Home() {
  if (!await sessionCookie()) return <SignInPage />;
  await pageAdmin();
  redirect("/users");
}
