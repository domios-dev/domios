import "server-only";
import { join } from "node:path";
import { firebaseBackend } from "./firebase";
import { FakeBackend } from "./fake";
import { LocalBackend } from "./local";
import type { Backend } from "../domain/model";

export function providerName(env = process.env) {
  const name = env.ADMIN_PROVIDER ?? (env.NODE_ENV === "production" || env.VERCEL ? "firebase" : "local");
  if (!["firebase", "local", "fake"].includes(name)) throw new Error("Unknown admin provider");
  if (name !== "firebase" && (env.NODE_ENV === "production" || env.VERCEL)) throw new Error("Development providers cannot run in production");
  return name;
}
let backend: Backend | undefined;
export function getBackend(): Backend {
  if (backend) return backend;
  const name = providerName();
  backend = name === "firebase" ? firebaseBackend() : name === "fake" ? new FakeBackend()
    : new LocalBackend(process.env.ADMIN_LOCAL_PATH ?? join(process.cwd(), ".local/admin.json"),
      process.env.ADMIN_LOCAL_SECRET ?? "local-development-secret", process.env.ADMIN_LOCAL_PASSWORD ?? "local-admin");
  return backend;
}
