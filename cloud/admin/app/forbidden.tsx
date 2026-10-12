import Link from "next/link";
export default function Forbidden() {
  return <><h1>Admin access required</h1><p>Sign in with an authorized admin account.</p><Link href="/signin">Sign in</Link></>;
}
