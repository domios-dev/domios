import type { AuthClient } from "../authClient";
import { firebaseClient, type FirebaseConfig } from "./firebase";
import { localClient } from "./local";

export type AuthConfig = ({provider:"firebase"} & FirebaseConfig) | {provider:"local"};
export function authClient(config: AuthConfig): Promise<AuthClient> {
  if (config.provider === "firebase") return firebaseClient(config);
  if (config.provider === "local") return Promise.resolve(localClient());
  throw new Error("Unknown auth adapter");
}
