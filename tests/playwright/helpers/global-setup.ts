import { ensureDevConsole } from "./dev-console";

export default async function globalSetup(): Promise<void> {
  await ensureDevConsole();
}
