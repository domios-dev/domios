import { notFound } from "next/navigation";
import { adminService, pageResult, sessionCookie } from "../../../src/server/admin";
import { changeEntitlement } from "../actions";
export default async function UserPage({ params }: { params: Promise<{ uid: string }> }) {
  const { uid } = await params;
  const cookie = await sessionCookie();
  const service = adminService();
  const detail = await pageResult(() => service.detail(cookie, uid));
  if (!detail) notFound();
  const { user, entitlement, customerUrl, audit } = detail;
  return <><h1>{user.email ?? user.uid}</h1><dl>
    <dt>UID</dt><dd>{user.uid}</dd><dt>Providers</dt><dd>{user.providers.join(", ") || "None"}</dd>
    <dt>Created</dt><dd>{user.createdAt}</dd><dt>Last sign-in</dt><dd>{user.lastSignInAt ?? "Never"}</dd>
    <dt>Disabled</dt><dd>{user.disabled ? "Yes" : "No"}</dd>
    <dt>Billing customer</dt><dd>{customerUrl ? <a href={customerUrl} target="_blank" rel="noopener noreferrer">Open billing customer</a> : "None"}</dd>
    </dl><h2>Entitlement</h2><pre>{JSON.stringify(entitlement, null, 2)}</pre>
    <h2>Change plan</h2><form action={changeEntitlement}>
      <input type="hidden" name="uid" value={uid} />
      <label>Plan<select name="plan">{service.plans.map(plan => <option key={plan}>{plan}</option>)}</select></label>
      <label>Reason<textarea name="reason" required maxLength={2000} /></label>
      <button name="action" value="grant">Grant plan</button><button name="action" value="revoke">Revoke plan</button>
    </form><h2>Audit history</h2><table><thead><tr><th scope="col">When</th><th scope="col">Actor</th><th scope="col">Action</th><th scope="col">Reason</th></tr></thead>
      <tbody>{audit.map(entry => <tr key={entry.id}><td>{new Date(entry.created_at * 1000).toISOString()}</td>
        <td>{entry.actor}</td><td>{entry.action}</td><td>{entry.reason}<details><summary>Before and after</summary><pre>{JSON.stringify({ before: entry.before, after: entry.after }, null, 2)}</pre></details></td></tr>)}</tbody></table></>;
}
