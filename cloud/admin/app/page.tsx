import { redirect } from "next/navigation";
import { pageAdmin } from "../src/server/admin";
export default async function Home() { await pageAdmin(); redirect("/users"); }
