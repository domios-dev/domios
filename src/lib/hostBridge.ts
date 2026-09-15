import { IS_WEB } from "./bridge";
import { currentToken } from "./webApi";

// A render with no Tauri runtime and no authenticated web session (tests, previews) has nothing to call or hold open.
export function hostBridgeAvailable(): boolean {
  if (typeof window === "undefined") return false;
  if (!IS_WEB) return true;
  return typeof localStorage !== "undefined" && currentToken() !== null;
}
