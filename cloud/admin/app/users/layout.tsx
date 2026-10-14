import Link from "next/link";
import type { ReactNode } from "react";
import { pageAdmin } from "../../src/server/admin";
import { signOut } from "./actions";
export const dynamic = "force-dynamic";
export default async function AdminLayout({ children }: { children: ReactNode }) {
  const actor = await pageAdmin();
  return <><nav><Link href="/users">Domios Admin</Link><span>{actor.uid}</span><form action={signOut}><button>Sign out</button></form></nav>{children}</>;
}
