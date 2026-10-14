import Link from "next/link";
import UserList from "./user-list";
import { adminService, pageAdmin, pageResult, sessionCookie } from "../../src/server/admin";
export default async function Users({ searchParams }: { searchParams: Promise<{ q?: string }> }) {
  await pageAdmin();
  const { q } = await searchParams;
  const cookie = await sessionCookie();
  const user = q ? await pageResult(() => adminService().search(cookie, q)) : null;
  return <><h1>Users</h1><form action="/users" method="get">
    <label>Email or uid<input name="q" defaultValue={q} required maxLength={320} /></label><button>Search</button>
    </form>{user ? <p><Link href={`/users/${encodeURIComponent(user.uid)}`}>{user.email ?? user.uid}</Link> · {user.uid}</p>
      : q && <p role="status">No user found.</p>}<UserList /></>;
}
